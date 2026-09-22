//! The fixture template store's address.
//!
//! A template is keyed by the file set it is built FROM; the store it stands in
//! is named by the steps it is built BY. Both halves have to be derived, or a
//! changed build recipe reads back a template the old steps made.

mod common;

use common::{recipe_key, templates, RECIPE};

#[test]
fn the_store_is_named_by_the_recipe_and_not_by_a_typed_number() {
    let name = templates()
        .file_name()
        .expect("the store directory has a name")
        .to_string_lossy()
        .into_owned();
    assert_eq!(
        name,
        format!("fleet-repo-templates-{}", recipe_key(RECIPE)),
        "the store's name carries the recipe's own key, so nothing has to be moved by hand"
    );
}

#[test]
fn an_edit_to_the_recipe_names_a_different_store() {
    let one_byte = format!("{RECIPE}\n");
    assert_ne!(
        recipe_key(RECIPE),
        recipe_key(&one_byte),
        "a one-byte edit to the recipe is a different key"
    );
    assert_eq!(
        recipe_key(RECIPE).len(),
        16,
        "the key is 16 hex, so it names one directory and not a path"
    );
}
