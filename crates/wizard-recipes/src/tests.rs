#![allow(clippy::unwrap_used)]

use std::fs;

use super::{PROJECT_RECIPES, Recipe, builtin, load};

fn texts(recipe: &Recipe, path: &str, source: &str) -> Vec<String> {
    recipe
        .facts(path, path, source)
        .into_iter()
        .map(|fact| format!("{}:{} {}", fact.extract, fact.line, fact.text))
        .collect()
}

fn named(name: &str) -> Recipe {
    let (recipes, errors) = builtin();
    assert_eq!(errors, []);
    recipes
        .into_iter()
        .find(|recipe| recipe.name == name)
        .unwrap()
}

#[test]
fn built_in_recipes_are_valid() {
    let (recipes, errors) = builtin();
    assert_eq!(errors, []);
    let names: Vec<&str> = recipes.iter().map(|recipe| recipe.name.as_str()).collect();
    assert_eq!(names, ["laravel", "react", "next", "expo"]);
}

#[test]
fn laravel_extracts_routes_relations_and_schema() {
    let laravel = named("laravel");
    let routes = "<?php\nuse App\\Http\\Controllers\\SongController;\n\nRoute::get('/songs', [SongController::class, 'index']);\nRoute::post(\"/songs/{id}/like\", LikeController::class);\n";
    assert_eq!(
        texts(&laravel, "routes/web.php", routes),
        [
            "routes:4 get /songs -> [SongController::class, 'index']",
            "routes:5 post /songs/{id}/like -> LikeController::class"
        ]
    );
    let model = "<?php\nclass Album extends Model\n{\n    public function songs(): HasMany\n    {\n        return $this->hasMany(Song::class);\n    }\n\n    public function artist()\n    {\n        return $this->belongsTo(Artist::class)->withTrashed();\n    }\n}\n";
    assert_eq!(
        texts(&laravel, "app/Models/Album.php", model),
        [
            "relations:4 songs: hasMany Song::class",
            "relations:9 artist: belongsTo Artist::class"
        ]
    );
    let migration = "<?php\nreturn new class extends Migration {\n    public function up(): void\n    {\n        Schema::create('songs', function (Blueprint $table) {\n            $table->string('title');\n            $table->index('title');\n        });\n    }\n};\n";
    assert_eq!(
        texts(
            &laravel,
            "database/migrations/2024_create_songs.php",
            migration
        ),
        [
            "schema:5 create table songs",
            "columns:6 column title: string"
        ]
    );
}

#[test]
fn react_extracts_components_and_hooks() {
    let react = named("react");
    let source = "export function Button({ label }: Props) {\n  return <b>{label}</b>;\n}\nconst Card = ({ title }) => <div>{title}</div>;\nexport const Memo = memo((props: P) => null);\nexport function useCart(id: string) {\n  return id;\n}\nfunction helper() {}\n";
    assert_eq!(
        texts(&react, "src/ui.tsx", source),
        [
            "components:1 <Button> ({ label }: Props)",
            "components:4 <Card> ({ title })",
            "components:5 <Memo> (props: P)",
            "hooks:6 useCart(id: string)"
        ]
    );
}

#[test]
fn next_maps_files_to_routes_and_finds_client_components() {
    let next = named("next");
    assert_eq!(
        texts(
            &next,
            "app/users/[id]/page.tsx",
            "export default function Page() {}\n"
        ),
        ["app-pages:1 page /users/[id]/"]
    );
    assert_eq!(texts(&next, "app/page.tsx", ""), ["app-pages:1 page /"]);
    assert_eq!(
        texts(&next, "pages/blog/index.tsx", ""),
        ["pages-router:1 page /blog/"]
    );
    assert_eq!(texts(&next, "pages/_app.tsx", ""), Vec::<String>::new());
    assert_eq!(
        texts(
            &next,
            "src/components/Menu.tsx",
            "'use client';\nexport function Menu() {}\n"
        ),
        ["client-components:1 use client"]
    );
}

#[test]
fn expo_maps_screens_and_navigation() {
    let expo = named("expo");
    assert_eq!(
        texts(&expo, "app/(tabs)/settings.tsx", ""),
        ["screens:1 screen /(tabs)/settings"]
    );
    assert_eq!(
        texts(&expo, "app/(tabs)/_layout.tsx", ""),
        ["layouts:1 layout /(tabs)/"]
    );
    let navigation = "export function App() {\n  return <Stack.Navigator><Stack.Screen name=\"Home\" component={Home} /></Stack.Navigator>;\n}\n";
    assert_eq!(
        texts(&expo, "src/App.tsx", navigation),
        ["navigation:2 screen Home"]
    );
}

#[test]
fn reports_every_problem_of_a_broken_recipe() {
    let text = "name = \"broken\"\n[detect]\n[[extract]]\nname = \"x\"\nglob = [\"[\"]\nlanguage = \"php\"\nquery = \"x.scm\"\ntemplate = \"{missing} {\"\n";
    let errors = Recipe::parse("broken", text, &|_| Some("(name) @found".to_owned())).unwrap_err();
    let messages: Vec<String> = errors.iter().map(ToString::to_string).collect();
    assert!(
        messages[0].starts_with("recipe `broken`, detect: needs at least one"),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|message| message.contains("extract \"x\", glob: `[`")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|message| message.contains("extract \"x\", template: unclosed")),
        "{messages:?}"
    );
    let unknown = Recipe::parse(
        "typo",
        "name = \"t\"\nfiles = []\n[detect]\nfiles = [\"a\"]\n",
        &|_| None,
    )
    .unwrap_err();
    assert!(
        unknown[0].message.contains("unknown field `files`"),
        "{unknown:?}"
    );
    let placeholder = "name = \"p\"\n[detect]\nfiles = [\"a\"]\n[[extract]]\nname = \"x\"\nglob = [\"*.php\"]\nlanguage = \"php\"\nquery = \"x.scm\"\ntemplate = \"{other}\"\n";
    let errors =
        Recipe::parse("p", placeholder, &|_| Some("(name) @found".to_owned())).unwrap_err();
    assert!(
        errors[0].message.contains("`{other}` is neither"),
        "{errors:?}"
    );
}

#[test]
fn project_recipes_replace_built_in_ones_and_are_detected() {
    let project = tempfile::tempdir().unwrap();
    let directory = project.path().join(PROJECT_RECIPES).join("laravel");
    fs::create_dir_all(&directory).unwrap();
    fs::write(
        directory.join("recipe.toml"),
        "name = \"laravel\"\n[detect]\nfiles = [\"artisan\"]\n[[extract]]\nname = \"commands\"\nglob = [\"app/Console/**/*.php\"]\npath_pattern = '(?P<name>[A-Za-z]+)\\.php$'\ntemplate = \"command {name}\"\n",
    )
    .unwrap();
    let (recipes, errors) = load(project.path());
    assert_eq!(errors, []);
    let laravel: Vec<&Recipe> = recipes
        .iter()
        .filter(|recipe| recipe.name == "laravel")
        .collect();
    assert_eq!(laravel.len(), 1);
    assert_eq!(laravel[0].extracts[0].name, "commands");
    assert!(!laravel[0].detects(project.path()));
    fs::write(project.path().join("artisan"), "").unwrap();
    assert!(laravel[0].detects(project.path()));
    assert!(laravel[0].reads("app/Console/Sync.php"));
    assert_eq!(
        texts(laravel[0], "app/Console/Sync.php", ""),
        ["commands:1 command Sync"]
    );
}

#[test]
fn the_worked_example_of_the_guide_works() {
    let guide = include_str!("../../../docs/recipes.md");
    let block = |start: &str, fence: &str| {
        let from = guide.find(start).unwrap();
        let body = &guide[from..];
        let open = body.find(fence).unwrap() + fence.len();
        let close = body[open..].find("```").unwrap();
        body[open..open + close].to_owned()
    };
    let recipe = block("### 1. Create the recipe", "```toml\n");
    let query = block("### 2. Write the query", "```scheme\n");
    let symfony = Recipe::parse("symfony", &recipe, &|name| {
        (name == "commands.scm").then(|| query.clone())
    })
    .unwrap();
    let source = "<?php\n\n#[AsCommand(name: 'app:sync-catalog', description: 'Synchronise the catalogue')]\nfinal class SyncCatalogCommand extends Command\n{\n}\n";
    assert_eq!(
        texts(&symfony, "src/Command/SyncCatalogCommand.php", source),
        ["commands:3 app:sync-catalog -> SyncCatalogCommand"]
    );
}
