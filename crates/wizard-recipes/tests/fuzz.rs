//! Recipes come from project files, so loading and applying them must never panic.

#![allow(clippy::unwrap_used)]

use proptest::prelude::{any, prop, proptest};
use wizard_recipes::{Recipe, builtin};

const SKELETON: &str = "name = \"fuzz\"\n[detect]\nfiles = [\"a\"]\n[[extract]]\nname = \"x\"\n";

proptest! {
    #[test]
    fn any_text_loads_or_fails_cleanly(text in any::<String>()) {
        let _ = Recipe::parse("fuzz", &text, &|_| None);
    }

    #[test]
    fn any_fields_load_or_fail_cleanly(
        glob in any::<String>(),
        pattern in any::<String>(),
        template in any::<String>(),
        query in any::<String>(),
    ) {
        let text = format!(
            "{SKELETON}glob = [{glob:?}]\npath_pattern = {pattern:?}\ntemplate = {template:?}\nlanguage = \"php\"\nquery = \"q.scm\"\n"
        );
        if let Ok(recipe) = Recipe::parse("fuzz", &text, &|_| Some(query.clone())) {
            let _ = recipe.check();
            let _ = recipe.facts("app/x.php", "app/x.php", "<?php echo 1;");
        }
    }

    #[test]
    fn built_in_recipes_survive_any_source(
        source in any::<String>(),
        path in prop::sample::select(vec![
            "routes/web.php",
            "app/Models/User.php",
            "database/migrations/x.php",
            "src/App.tsx",
            "src/hooks.ts",
            "app/users/page.tsx",
            "app/(tabs)/index.tsx",
            "lib/x.js",
        ]),
    ) {
        let (recipes, errors) = builtin();
        assert_eq!(errors, []);
        for recipe in &recipes {
            let _ = recipe.facts(path, path, &source);
        }
    }
}
