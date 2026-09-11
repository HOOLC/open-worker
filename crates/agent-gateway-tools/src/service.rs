//! Service tool contracts. Operational guidance lives in the bundled skill.
use super::Kind;
use serde_json::{json, Value};
type Definition = (Kind, &'static str, &'static str, Value, Vec<&'static str>);
pub(super) fn definitions() -> Vec<Definition> {
    let id = || json!({"type":"string","minLength":1,"maxLength":128});
    let name = || json!({"type":"string","minLength":1,"maxLength":80});
    let port = || json!({"type":"integer","minimum":1,"maximum":65535});
    let mutation = || json!({"id":id(),"expected_revision":{"type":"integer","minimum":0}});
    vec![
        (Kind::ServiceOp("start"), "service.start", "Create or resume a Session-owned managed service on this execution node. command is an argv array; cwd defaults to the Session workspace. Saves the running intent across Gateway restarts. Does not enable sharing and returns before HTTP readiness. Reusing a name preserves its id; changing its command, cwd or port requires expected_revision. Returns state, readiness and log paths.", json!({"name":name(),"port":port(),"command":{"type":"array","minItems":1,"maxItems":128,"items":{"type":"string"}},"cwd":{"type":"string"},"expected_revision":{"type":"integer","minimum":0}}), vec!["name","port","command"]),
        (Kind::ServiceOp("attach"), "service.attach", "Register an externally managed HTTP service on this node's loopback port. Does not start, stop, share, or capture output from the external process. Returns a stable Session-owned id. Identical name/port reuses the record; conflicting configuration is rejected.", json!({"name":name(),"port":port()}), vec!["name","port"]),
        (Kind::ServiceOp("list"), "service.list", "List this Session's registered services, including stopped and unshared records. Returns saved configuration, process state, sharing state and node identity; does not probe HTTP readiness or read log files.", json!({}), vec![]),
        (Kind::ServiceOp("inspect"), "service.inspect", "Inspect a Session-owned service by id. Returns process state, TCP readiness, last exit code/error, revision, sharing URL and node-qualified stdout/stderr file paths for managed services. External services have no captured logs. Does not read log contents or change desired state.", json!({"id":id()}), vec!["id"]),
        (Kind::ServiceOp("restart"), "service.restart", "Stop the managed process group and launch the saved command again, including for a stopped service. Preserves id, sharing state and logs; persists the running intent. External services are rejected. expected_revision, when supplied, rejects stale state.", mutation(), vec!["id"]),
        (Kind::ServiceOp("stop"), "service.stop", "Stop the managed process group and persist stopped intent, preventing automatic launch after Gateway restart. Preserves id, configuration, sharing setting and log files; existing service streams close. External services are rejected.", mutation(), vec!["id"]),
        (Kind::ServiceOp("share"), "service.share", "Enable persistent Mesh access to a registered service for currently authorized client peers and return its stable URL. Does not start the process or require readiness. Repeated sharing preserves the URL.", mutation(), vec!["id"]),
        (Kind::ServiceOp("unshare"), "service.unshare", "Disable Mesh access and close active streams. Preserves the service id, process, configuration and logs. The disabled setting survives Gateway restart; sharing again reuses the same URL.", mutation(), vec!["id"]),
    ]
}
