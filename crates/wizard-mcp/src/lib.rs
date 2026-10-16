//! MCP server of Context Wizard.
//!
//! The server answers Claude's questions about the project from the index: the overview, the
//! ranked map, symbol definitions and references, the files relevant to a task, the impact of a
//! change and the outline of a file. Relevance and impact also draw on what past sessions
//! learned about which files are used together. Before answering, it brings the index up to date
//! when the last refresh is older than two seconds, so answers never describe stale code.
//! Nothing is indexed for a project that has not enabled Context Wizard.

#![allow(
    clippy::unused_async_trait_impl,
    reason = "the rmcp tool_router macro generates a ServerHandler implementation it lints"
)]

use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rmcp::handler::server::wrapper::Parameters;
use rmcp::{ServiceExt, tool, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;
use wizard_core::decision;
use wizard_git::{History, HistoryOptions};
use wizard_index::build::{BuildOptions, build};
use wizard_index::learned::Learned;
use wizard_index::map::{L1_BUDGET, map};
use wizard_index::overview::overview;
use wizard_index::query;
use wizard_store::{Index, StoreError};

const REFRESH_INTERVAL: Duration = Duration::from_secs(2);
const HISTORY_LIFETIME: Duration = Duration::from_secs(300);

/// Serves the tools of `project` over standard input and output until the client disconnects.
///
/// # Errors
///
/// Returns an error when the MCP session cannot be initialised or fails.
pub async fn serve_stdio(
    project: PathBuf,
    options: BuildOptions,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let service = WizardServer::new(project, options)
        .serve(rmcp::transport::stdio())
        .await?;
    service.waiting().await?;
    Ok(())
}

#[derive(Debug, Default)]
struct Freshness {
    refreshed: Option<Instant>,
    history: Option<(Instant, Option<History>)>,
}

/// The MCP server of one project.
#[derive(Debug, Clone)]
pub struct WizardServer {
    project: PathBuf,
    options: BuildOptions,
    freshness: Arc<Mutex<Freshness>>,
}

/// A symbol name.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct NameParameter {
    /// Name of a function, class, method, type, constant or variable, as written in the code.
    pub name: String,
}

/// A file of the project.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct PathParameter {
    /// Path relative to the project root, with `/` separators, such as `src/app/main.ts`.
    pub path: String,
}

/// A task described in natural language.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct TaskParameter {
    /// What needs to be done or understood, with the specific words a developer would use.
    pub task: String,
}

#[tool_router(server_handler)]
impl WizardServer {
    /// Creates the server of `project`, building its index with `options`.
    #[must_use]
    pub fn new(project: PathBuf, options: BuildOptions) -> Self {
        Self {
            project,
            options,
            freshness: Arc::new(Mutex::new(Freshness::default())),
        }
    }

    #[tool(
        description = "Short overview of the project: stack, sub-projects, entry points, commands and folder roles. Use it first in an unfamiliar project."
    )]
    async fn overview(&self) -> String {
        self.answer(false, |project, index, _, _| overview(project, index))
            .await
    }

    #[tool(
        description = "Map of the most used definitions with signatures and line numbers, grouped by file. Use it to find where the important code lives before reading files."
    )]
    async fn map(&self) -> String {
        self.answer(false, |_, index, _, _| map(index, L1_BUDGET))
            .await
    }

    #[tool(
        description = "Where a function, class, method, type or constant is defined, with its signature and line. Faster and cheaper than grep for a name."
    )]
    async fn find_symbol(&self, Parameters(parameter): Parameters<NameParameter>) -> String {
        self.answer(false, move |_, index, _, _| {
            query::find_symbol(index, &parameter.name)
        })
        .await
    }

    #[tool(
        description = "Every place a name is used, grouped by file with line numbers, plus where it is defined. Use it before renaming or changing a signature."
    )]
    async fn refs(&self, Parameters(parameter): Parameters<NameParameter>) -> String {
        self.answer(false, move |_, index, _, _| {
            query::refs(index, &parameter.name)
        })
        .await
    }

    #[tool(
        description = "The files most relevant to a task described in words, with their matching definitions. Use it at the start of a task to know which files to read."
    )]
    async fn slice(&self, Parameters(parameter): Parameters<TaskParameter>) -> String {
        self.answer(true, move |_, index, history, learned| {
            query::slice(index, &parameter.task, history, learned)
        })
        .await
    }

    #[tool(
        description = "Files affected by a change to a file: the files that use it, directly and indirectly, and the files that usually change with it in git history."
    )]
    async fn impact(&self, Parameters(parameter): Parameters<PathParameter>) -> String {
        self.answer(true, move |_, index, history, learned| {
            query::impact(index, &parameter.path, history, learned)
        })
        .await
    }

    #[tool(
        description = "Outline of a file without reading it: its definitions with lines, the files it uses and the files that use it."
    )]
    async fn explain(&self, Parameters(parameter): Parameters<PathParameter>) -> String {
        self.answer(false, move |_, index, _, _| {
            query::explain(index, &parameter.path)
        })
        .await
    }
}

impl WizardServer {
    async fn answer<F>(&self, needs_history: bool, question: F) -> String
    where
        F: FnOnce(&Path, &Index, Option<&History>, &Learned) -> Result<String, StoreError>
            + Send
            + 'static,
    {
        let server = self.clone();
        tokio::task::spawn_blocking(move || server.answer_now(needs_history, question))
            .await
            .unwrap_or_else(|_| "Context Wizard could not answer this request.".to_owned())
    }

    fn answer_now<F>(&self, needs_history: bool, question: F) -> String
    where
        F: FnOnce(&Path, &Index, Option<&History>, &Learned) -> Result<String, StoreError>,
    {
        if !decision::load(&self.project).is_ok_and(|state| state.is_enabled()) {
            return "Context Wizard is not enabled for this project. The user can enable it with /context-wizard:on.".to_owned();
        }
        self.refresh_if_stale();
        let (history, learned) = if needs_history {
            (
                self.history(),
                Learned::load(&self.project, wizard_learn::now()),
            )
        } else {
            (None, Learned::default())
        };
        match Index::open_project(&self.project)
            .and_then(|index| question(&self.project, &index, history.as_ref(), &learned))
        {
            Ok(text) => text,
            Err(error) => format!("Context Wizard could not read its index: {error}"),
        }
    }

    fn refresh_if_stale(&self) {
        let stale = self.freshness.lock().map_or(true, |freshness| {
            freshness
                .refreshed
                .is_none_or(|refreshed| refreshed.elapsed() >= REFRESH_INTERVAL)
        });
        if stale
            && build(&self.project, &self.options).is_ok()
            && let Ok(mut freshness) = self.freshness.lock()
        {
            freshness.refreshed = Some(Instant::now());
        }
    }

    fn history(&self) -> Option<History> {
        if let Ok(freshness) = self.freshness.lock()
            && let Some((read, history)) = &freshness.history
            && read.elapsed() < HISTORY_LIFETIME
        {
            return history.clone();
        }
        let history = wizard_git::history(&self.project, &HistoryOptions::default());
        if let Ok(mut freshness) = self.freshness.lock() {
            freshness.history = Some((Instant::now(), history.clone()));
        }
        history
    }
}
