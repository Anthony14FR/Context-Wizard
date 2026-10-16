//! Recipes compiled into the binary, from the `recipes` directory of the repository.

type Files = &'static [(&'static str, &'static str)];

/// Name, `recipe.toml` and query files of each built-in recipe.
pub(crate) const RECIPES: &[(&str, &str, Files)] = &[
    (
        "laravel",
        include_str!("../../../recipes/laravel/recipe.toml"),
        &[
            (
                "routes.scm",
                include_str!("../../../recipes/laravel/routes.scm"),
            ),
            (
                "relations.scm",
                include_str!("../../../recipes/laravel/relations.scm"),
            ),
            (
                "migrations.scm",
                include_str!("../../../recipes/laravel/migrations.scm"),
            ),
            (
                "columns.scm",
                include_str!("../../../recipes/laravel/columns.scm"),
            ),
        ],
    ),
    (
        "react",
        include_str!("../../../recipes/react/recipe.toml"),
        &[
            (
                "components.scm",
                include_str!("../../../recipes/react/components.scm"),
            ),
            (
                "hooks.scm",
                include_str!("../../../recipes/react/hooks.scm"),
            ),
        ],
    ),
    (
        "next",
        include_str!("../../../recipes/next/recipe.toml"),
        &[(
            "directive.scm",
            include_str!("../../../recipes/next/directive.scm"),
        )],
    ),
    (
        "expo",
        include_str!("../../../recipes/expo/recipe.toml"),
        &[(
            "navigation.scm",
            include_str!("../../../recipes/expo/navigation.scm"),
        )],
    ),
];
