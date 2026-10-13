use super::*;

#[test]
fn all_embedded_templates_parse() {
    let tera = load_templates().expect("templates must parse");
    let names: Vec<&str> = tera.get_template_names().collect();
    assert!(names.contains(&"_base.html"), "{names:?}");
    assert!(names.contains(&"catalog/detail.html"), "{names:?}");
}
