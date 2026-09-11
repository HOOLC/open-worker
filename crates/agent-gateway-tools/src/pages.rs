use super::Kind;
use serde_json::{json, Value};
type Definition = (Kind, &'static str, &'static str, Value, Vec<&'static str>);
pub(super) fn definitions() -> Vec<Definition> {
    let page = || json!({"title":{"type":"string","minLength":1,"maxLength":160},"url":{"type":"string","maxLength":8192},"description":{"type":"string","maxLength":2048}});
    vec![
        (Kind::Page("publish"),"page.publish","Publish a page as a persistent human-facing application in the Mesh application list, only when the user wants to keep using it beyond this Task. Existing service sharing or runtime duration does not imply publication. Does not start or share the service. The same URL reuses its entry.",page(),vec!["title","url"]),
        (Kind::Page("unpublish"),"page.unpublish","Remove an application published by this Session. Does not stop its service or remove previously delivered conversation references.",json!({"id":{"type":"string","maxLength":128}}),vec!["id"]),
    ]
}
