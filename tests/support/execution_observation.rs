//! Test-only observations of the transport boundary, before output policy or IR conversion.

use diplodocus::ir::CodeCell;
use jupyter_protocol::{ExecuteReply, JupyterMessageContent, Stdio};
use serde_json::{Value, json};

#[derive(Default)]
pub struct ExecutionObservation {
    display_ids: Vec<String>,
}

impl ExecutionObservation {
    pub fn cell(
        &mut self,
        ordinal: usize,
        cell: &CodeCell,
        reply: &ExecuteReply,
        outputs: &[JupyterMessageContent],
    ) -> Value {
        let outputs = outputs.iter().map(|output| match output {
            JupyterMessageContent::StreamContent(stream) => json!({
                "kind": "stream",
                "name": match stream.name { Stdio::Stdout => "stdout", Stdio::Stderr => "stderr" },
                "text": stream.text,
            }),
            JupyterMessageContent::DisplayData(display) => json!({
                "kind": "display", "data": display.data, "metadata": display.metadata,
                "display_ref": self.display_ref(display.transient.as_ref().and_then(|value| value.display_id.as_deref())),
            }),
            JupyterMessageContent::UpdateDisplayData(display) => json!({
                "kind": "update", "data": display.data, "metadata": display.metadata,
                "display_ref": self.display_ref(display.transient.display_id.as_deref()),
            }),
            JupyterMessageContent::ExecuteResult(result) => json!({
                "kind": "result", "data": result.data, "metadata": result.metadata,
            }),
            JupyterMessageContent::ErrorOutput(error) => json!({
                "kind": "error", "name": error.ename, "value": error.evalue,
                "traceback": error.traceback.iter().map(|line| traceback_line(line, ordinal)).collect::<Vec<_>>(),
            }),
            other => panic!("unrepresented execution output: {other:?}"),
        }).collect::<Vec<_>>();
        json!({
            "ordinal": ordinal,
            "source": cell.source,
            "span": cell.span,
            "options": cell.resolved_options,
            "reply_status": reply.status,
            "reply_error": reply.error.as_ref().map(|error| json!({
                "name": error.ename, "value": error.evalue,
                "traceback": error.traceback.iter().map(|line| traceback_line(line, ordinal)).collect::<Vec<_>>(),
            })),
            "outputs": outputs,
        })
    }

    fn display_ref(&mut self, id: Option<&str>) -> Option<usize> {
        id.map(|id| {
            if let Some(index) = self.display_ids.iter().position(|value| value == id) {
                index
            } else {
                self.display_ids.push(id.to_owned());
                self.display_ids.len() - 1
            }
        })
    }
}

fn traceback_line(line: &str, ordinal: usize) -> String {
    let mut chars = line.chars().peekable();
    let mut plain = String::new();
    while let Some(character) = chars.next() {
        if character == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for code in chars.by_ref() {
                if ('@'..='~').contains(&code) {
                    break;
                }
            }
        } else {
            plain.push(character);
        }
    }
    // Kernel execution counts are session identities, while authored ordinals survive replay.
    plain.replace(
        &format!("Cell In[{}]", ordinal + 1),
        &format!("Cell <authored:{ordinal}>"),
    )
}
