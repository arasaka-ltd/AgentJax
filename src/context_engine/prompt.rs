use serde::{Deserialize, Serialize};

use crate::{
    api::SessionMessage,
    builtin::tools::ToolDescriptor,
    config::{WorkspaceDocument, WorkspaceIdentityPack},
    context_engine::AssembledContext,
    domain::{ContextBlock, ContextBlockKind, Freshness},
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PromptDocumentKind {
    Agent,
    Soul,
    User,
    Mission,
    Rules,
    Router,
    Memory,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PromptSectionKind {
    Identity,
    Rules,
    Mission,
    Router,
    UserProfile,
    Memory,
    Knowledge,
    Task,
    Conversation,
    Runtime,
    Misc,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PromptDocument {
    pub document_id: String,
    pub kind: PromptDocumentKind,
    pub source_path: String,
    pub sections: Vec<PromptSection>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PromptSection {
    pub section_id: String,
    pub title: String,
    pub kind: PromptSectionKind,
    pub fragments: Vec<PromptFragment>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PromptFragment {
    pub source_file: String,
    pub section_title: String,
    pub content: String,
    pub priority: u32,
    pub freshness: Option<Freshness>,
}

#[derive(Debug, Clone)]
pub struct PromptRenderRequest {
    pub prompt_documents: Vec<PromptDocument>,
    pub assembled_context: AssembledContext,
    pub tools: Vec<ToolDescriptor>,
    pub conversation_messages: Vec<SessionMessage>,
    pub allow_tool_calls: bool,
}

pub fn parse_workspace_prompt_documents(identity: &WorkspaceIdentityPack) -> Vec<PromptDocument> {
    vec![
        parse_workspace_document("agent", PromptDocumentKind::Agent, &identity.agent),
        parse_workspace_document("soul", PromptDocumentKind::Soul, &identity.soul),
        parse_workspace_document("user", PromptDocumentKind::User, &identity.user),
        parse_workspace_document("mission", PromptDocumentKind::Mission, &identity.mission),
        parse_workspace_document("rules", PromptDocumentKind::Rules, &identity.rules),
        parse_workspace_document("router", PromptDocumentKind::Router, &identity.router),
        parse_workspace_document("memory", PromptDocumentKind::Memory, &identity.memory),
    ]
}

pub fn render_prompt_xml(request: PromptRenderRequest) -> String {
    let mut xml = String::new();
    xml.push_str("<agentjax_prompt version=\"v1\">\n");
    render_identity(&mut xml, &request.prompt_documents);
    render_memory(
        &mut xml,
        &request.prompt_documents,
        &request.assembled_context.blocks,
    );
    render_knowledge(&mut xml, &request.assembled_context.blocks);
    render_tools(&mut xml, &request.tools, request.allow_tool_calls);
    render_task_state(&mut xml, &request.assembled_context.blocks);
    render_runtime(&mut xml, &request.assembled_context);
    render_conversation(
        &mut xml,
        &request.assembled_context.blocks,
        &request.conversation_messages,
    );
    xml.push_str("</agentjax_prompt>");
    xml
}

fn parse_workspace_document(
    document_id: &str,
    kind: PromptDocumentKind,
    document: &WorkspaceDocument,
) -> PromptDocument {
    let mut sections = Vec::new();
    let mut current_title: Option<String> = None;
    let mut current_lines = Vec::new();

    for line in document.content.lines() {
        if let Some(title) = line.strip_prefix("## ") {
            push_section(
                &mut sections,
                &kind,
                &document.path.display().to_string(),
                current_title.take(),
                std::mem::take(&mut current_lines),
            );
            current_title = Some(title.trim().to_string());
        } else {
            current_lines.push(line);
        }
    }

    push_section(
        &mut sections,
        &kind,
        &document.path.display().to_string(),
        current_title,
        current_lines,
    );

    if sections.is_empty() && !document.content.trim().is_empty() {
        sections.push(PromptSection {
            section_id: format!("{document_id}.misc"),
            title: "misc".into(),
            kind: PromptSectionKind::Misc,
            fragments: vec![PromptFragment {
                source_file: document.path.display().to_string(),
                section_title: "misc".into(),
                content: document.content.trim().into(),
                priority: 50,
                freshness: None,
            }],
        });
    }

    PromptDocument {
        document_id: document_id.into(),
        kind,
        source_path: document.path.display().to_string(),
        sections,
    }
}

fn push_section(
    sections: &mut Vec<PromptSection>,
    kind: &PromptDocumentKind,
    source_file: &str,
    title: Option<String>,
    lines: Vec<&str>,
) {
    let content = lines.join("\n").trim().to_string();
    let Some(title) = title else {
        return;
    };
    if content.is_empty() {
        return;
    }
    let normalized_title = title.trim().to_string();
    sections.push(PromptSection {
        section_id: format!(
            "{}.{}",
            prompt_document_label(kind),
            slugify(&normalized_title)
        ),
        kind: normalize_section_kind(kind, &normalized_title),
        title: normalized_title.clone(),
        fragments: vec![PromptFragment {
            source_file: source_file.into(),
            section_title: normalized_title,
            content,
            priority: section_priority(kind),
            freshness: None,
        }],
    });
}

fn normalize_section_kind(kind: &PromptDocumentKind, title: &str) -> PromptSectionKind {
    let title = title.to_ascii_lowercase();
    match kind {
        PromptDocumentKind::Agent | PromptDocumentKind::Soul => PromptSectionKind::Identity,
        PromptDocumentKind::Mission => PromptSectionKind::Mission,
        PromptDocumentKind::Rules => PromptSectionKind::Rules,
        PromptDocumentKind::Router => PromptSectionKind::Router,
        PromptDocumentKind::User => PromptSectionKind::UserProfile,
        PromptDocumentKind::Memory => {
            if matches!(
                title.as_str(),
                "stable facts" | "preferences" | "long-term decisions" | "open loops"
            ) {
                PromptSectionKind::Memory
            } else {
                PromptSectionKind::Misc
            }
        }
    }
}

fn section_priority(kind: &PromptDocumentKind) -> u32 {
    match kind {
        PromptDocumentKind::Agent | PromptDocumentKind::Soul => 10,
        PromptDocumentKind::Mission => 20,
        PromptDocumentKind::Rules => 30,
        PromptDocumentKind::Router => 40,
        PromptDocumentKind::User => 50,
        PromptDocumentKind::Memory => 60,
    }
}

fn prompt_document_label(kind: &PromptDocumentKind) -> &'static str {
    match kind {
        PromptDocumentKind::Agent => "agent",
        PromptDocumentKind::Soul => "soul",
        PromptDocumentKind::User => "user",
        PromptDocumentKind::Mission => "mission",
        PromptDocumentKind::Rules => "rules",
        PromptDocumentKind::Router => "router",
        PromptDocumentKind::Memory => "memory",
    }
}

fn render_identity(xml: &mut String, documents: &[PromptDocument]) {
    xml.push_str("  <identity>\n");
    render_document_sections(xml, "agent", documents, PromptDocumentKind::Agent);
    render_document_sections(xml, "soul", documents, PromptDocumentKind::Soul);
    render_document_sections(xml, "mission", documents, PromptDocumentKind::Mission);
    render_document_sections(xml, "rules", documents, PromptDocumentKind::Rules);
    render_document_sections(xml, "router", documents, PromptDocumentKind::Router);
    render_document_sections(xml, "user", documents, PromptDocumentKind::User);
    xml.push_str("  </identity>\n");
}

fn render_document_sections(
    xml: &mut String,
    tag: &str,
    documents: &[PromptDocument],
    kind: PromptDocumentKind,
) {
    let Some(document) = documents.iter().find(|document| document.kind == kind) else {
        return;
    };
    xml.push_str("    <");
    xml.push_str(tag);
    xml.push_str(" source=\"");
    xml.push_str(&escape_xml(&document.source_path));
    xml.push_str("\">\n");
    for section in &document.sections {
        render_section(xml, section, 3);
    }
    xml.push_str("    </");
    xml.push_str(tag);
    xml.push_str(">\n");
}

fn render_memory(xml: &mut String, documents: &[PromptDocument], blocks: &[ContextBlock]) {
    xml.push_str("  <memory>\n");
    if let Some(memory) = documents
        .iter()
        .find(|document| document.kind == PromptDocumentKind::Memory)
    {
        for section in &memory.sections {
            for fragment in &section.fragments {
                xml.push_str("    <item kind=\"");
                xml.push_str(&escape_xml(&slugify(&section.title)));
                xml.push_str("\" source=\"");
                xml.push_str(&escape_xml(&fragment.source_file));
                xml.push_str("\">");
                xml.push_str(&escape_xml(&fragment.content));
                xml.push_str("</item>\n");
            }
        }
    }
    for block in blocks
        .iter()
        .filter(|block| block.kind == ContextBlockKind::Memory)
        .filter(|block| {
            !matches!(
                block.source,
                crate::domain::ContextSource::WorkspaceFile { .. }
            )
        })
    {
        render_context_item(xml, "item", Some("recall"), block);
    }
    xml.push_str("  </memory>\n");
}

fn render_knowledge(xml: &mut String, blocks: &[ContextBlock]) {
    xml.push_str("  <knowledge>\n");
    for block in blocks
        .iter()
        .filter(|block| block.kind == ContextBlockKind::RetrievedKnowledge)
    {
        render_context_item(xml, "item", Some("retrieved"), block);
    }
    xml.push_str("  </knowledge>\n");
}

fn render_tools(xml: &mut String, tools: &[ToolDescriptor], allow_tool_calls: bool) {
    xml.push_str("  <tools>\n");
    if allow_tool_calls {
        xml.push_str("    <tool_call_protocol mode=\"agentjax.v1.structured\">");
        xml.push_str(
            "Use tools through the runtime's structured tool-calling protocol when needed. Prefer structured tool requests over describing tool use in prose. Compatibility text fallback may be supported by the provider adapter during migration.",
        );
        xml.push_str("</tool_call_protocol>\n");
    }
    for tool in tools {
        xml.push_str("    <tool name=\"");
        xml.push_str(&escape_xml(&tool.name));
        xml.push_str("\" idempotent=\"");
        xml.push_str(if tool.idempotent { "true" } else { "false" });
        xml.push_str("\" default_timeout_secs=\"");
        xml.push_str(&tool.default_timeout_secs.to_string());
        xml.push_str("\">\n");
        xml.push_str("      <description>");
        xml.push_str(&escape_xml(&tool.description));
        xml.push_str("</description>\n");
        xml.push_str("      <when_to_use>");
        xml.push_str(&escape_xml(&tool.when_to_use));
        xml.push_str("</when_to_use>\n");
        xml.push_str("      <when_not_to_use>");
        xml.push_str(&escape_xml(&tool.when_not_to_use));
        xml.push_str("</when_not_to_use>\n");
        xml.push_str("      <arguments_schema>");
        xml.push_str(&escape_xml(&tool.arguments_schema.to_string()));
        xml.push_str("</arguments_schema>\n");
        xml.push_str("    </tool>\n");
    }
    xml.push_str("  </tools>\n");
}

fn render_task_state(xml: &mut String, blocks: &[ContextBlock]) {
    xml.push_str("  <task_state>\n");
    for block in blocks.iter().filter(|block| {
        matches!(
            block.kind,
            ContextBlockKind::TaskPlan | ContextBlockKind::Summary | ContextBlockKind::Checkpoint
        )
    }) {
        render_context_item(xml, "item", Some("task"), block);
    }
    xml.push_str("  </task_state>\n");
}

fn render_runtime(xml: &mut String, assembled: &AssembledContext) {
    xml.push_str("  <runtime>\n");
    xml.push_str("    <token_breakdown total=\"");
    xml.push_str(&assembled.token_breakdown.total.to_string());
    xml.push_str("\" stable_docs=\"");
    xml.push_str(&assembled.token_breakdown.stable_docs.to_string());
    xml.push_str("\" runtime=\"");
    xml.push_str(&assembled.token_breakdown.runtime.to_string());
    xml.push_str("\" summaries=\"");
    xml.push_str(&assembled.token_breakdown.summaries.to_string());
    xml.push_str("\" fresh_tail=\"");
    xml.push_str(&assembled.token_breakdown.fresh_tail.to_string());
    xml.push_str("\" retrieval=\"");
    xml.push_str(&assembled.token_breakdown.retrieval.to_string());
    xml.push_str("\" />\n");
    for directive in &assembled.system_prompt_additions {
        xml.push_str("    <directive>");
        xml.push_str(&escape_xml(directive));
        xml.push_str("</directive>\n");
    }
    xml.push_str("  </runtime>\n");
}

fn render_conversation(xml: &mut String, blocks: &[ContextBlock], messages: &[SessionMessage]) {
    xml.push_str("  <conversation>\n");
    for block in blocks
        .iter()
        .filter(|block| block.kind == ContextBlockKind::RecentEvent)
    {
        render_context_item(xml, "recent_transcript", None, block);
    }
    for message in messages {
        render_message(xml, message);
    }
    xml.push_str("  </conversation>\n");
}

fn render_message(xml: &mut String, message: &SessionMessage) {
    let kind = message.normalized_kind();
    xml.push_str("    <message kind=\"");
    xml.push_str(kind.as_role_str());
    xml.push_str("\">\n");
    xml.push_str("      <meta>\n");
    render_meta_field(xml, "message_id", message.meta.message_id.as_deref());
    render_meta_field(xml, "session_id", message.meta.session_id.as_deref());
    render_meta_field(xml, "channel", message.meta.channel.as_deref());
    render_meta_field(xml, "surface", message.meta.surface.as_deref());
    render_meta_field(xml, "actor_id", message.meta.actor_id.as_deref());
    if let Some(timestamp) = message.meta.timestamp.as_ref() {
        xml.push_str("        <timestamp>");
        xml.push_str(&escape_xml(&timestamp.to_rfc3339()));
        xml.push_str("</timestamp>\n");
    }
    render_meta_field(xml, "locale", message.meta.locale.as_deref());
    if !message.meta.extra.is_empty() {
        xml.push_str("        <extra>\n");
        for (key, value) in &message.meta.extra {
            xml.push_str("          <item key=\"");
            xml.push_str(&escape_xml(key));
            xml.push_str("\">");
            xml.push_str(&escape_xml(value));
            xml.push_str("</item>\n");
        }
        xml.push_str("        </extra>\n");
    }
    xml.push_str("      </meta>\n");
    xml.push_str("      <content>");
    xml.push_str(&escape_xml(&message.content));
    xml.push_str("</content>\n");
    if !message.annotations.is_empty() {
        xml.push_str("      <annotations>\n");
        for annotation in &message.annotations {
            xml.push_str("        <annotation kind=\"");
            xml.push_str(&escape_xml(&annotation.kind));
            xml.push_str("\">");
            xml.push_str(&escape_xml(&annotation.value));
            xml.push_str("</annotation>\n");
        }
        xml.push_str("      </annotations>\n");
    }
    xml.push_str("    </message>\n");
}

fn render_meta_field(xml: &mut String, tag: &str, value: Option<&str>) {
    let Some(value) = value else {
        return;
    };
    xml.push_str("        <");
    xml.push_str(tag);
    xml.push('>');
    xml.push_str(&escape_xml(value));
    xml.push_str("</");
    xml.push_str(tag);
    xml.push_str(">\n");
}

fn render_section(xml: &mut String, section: &PromptSection, indent_level: usize) {
    let indent = "  ".repeat(indent_level);
    xml.push_str(&indent);
    xml.push_str("<section title=\"");
    xml.push_str(&escape_xml(&section.title));
    xml.push_str("\" kind=\"");
    xml.push_str(match section.kind {
        PromptSectionKind::Identity => "identity",
        PromptSectionKind::Rules => "rules",
        PromptSectionKind::Mission => "mission",
        PromptSectionKind::Router => "router",
        PromptSectionKind::UserProfile => "user_profile",
        PromptSectionKind::Memory => "memory",
        PromptSectionKind::Knowledge => "knowledge",
        PromptSectionKind::Task => "task",
        PromptSectionKind::Conversation => "conversation",
        PromptSectionKind::Runtime => "runtime",
        PromptSectionKind::Misc => "misc",
    });
    xml.push_str("\">\n");
    for fragment in &section.fragments {
        xml.push_str(&indent);
        xml.push_str("  <fragment source=\"");
        xml.push_str(&escape_xml(&fragment.source_file));
        xml.push_str("\">");
        xml.push_str(&escape_xml(&fragment.content));
        xml.push_str("</fragment>\n");
    }
    xml.push_str(&indent);
    xml.push_str("</section>\n");
}

fn render_context_item(xml: &mut String, tag: &str, kind: Option<&str>, block: &ContextBlock) {
    xml.push_str("    <");
    xml.push_str(tag);
    if let Some(kind) = kind {
        xml.push_str(" kind=\"");
        xml.push_str(kind);
        xml.push('"');
    }
    xml.push_str(" block_id=\"");
    xml.push_str(&escape_xml(&block.block_id));
    xml.push_str("\">");
    xml.push_str(&escape_xml(&block.content));
    xml.push_str("</");
    xml.push_str(tag);
    xml.push_str(">\n");
}

fn escape_xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn slugify(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        parse_workspace_prompt_documents, render_prompt_xml, PromptDocumentKind,
        PromptRenderRequest,
    };
    use crate::{
        api::{SessionMessage, SessionMessageAnnotation},
        builtin::tools::ToolDescriptor,
        config::{WorkspaceDocument, WorkspaceIdentityPack},
        context_engine::assembler::{AssembledContext, TokenBreakdown},
        domain::{ContextBlock, ContextBlockKind, ContextSource},
    };

    #[test]
    fn parses_workspace_markdown_into_prompt_documents() {
        let identity = identity_pack();
        let documents = parse_workspace_prompt_documents(&identity);

        let agent = documents
            .iter()
            .find(|document| document.kind == PromptDocumentKind::Agent)
            .unwrap();
        assert_eq!(agent.sections.len(), 2);
        assert_eq!(agent.sections[0].title, "Role");
        assert_eq!(agent.sections[1].title, "Working Style");

        let memory = documents
            .iter()
            .find(|document| document.kind == PromptDocumentKind::Memory)
            .unwrap();
        assert_eq!(memory.sections[0].title, "Stable Facts");
    }

    #[test]
    fn renders_xml_prompt_with_structured_messages() {
        let identity = identity_pack();
        let xml = render_prompt_xml(PromptRenderRequest {
            prompt_documents: parse_workspace_prompt_documents(&identity),
            assembled_context: AssembledContext {
                blocks: vec![
                    ContextBlock {
                        block_id: "transcript.recent".into(),
                        kind: ContextBlockKind::RecentEvent,
                        source: ContextSource::EventLog {
                            event_id: "evt_1".into(),
                        },
                        priority: 100,
                        token_estimate: Some(10),
                        freshness: None,
                        confidence: None,
                        content: "user: hi\nassistant: hello".into(),
                    },
                    ContextBlock {
                        block_id: "knowledge.1".into(),
                        kind: ContextBlockKind::RetrievedKnowledge,
                        source: ContextSource::Memory {
                            memory_ref: "knowledge/docs".into(),
                        },
                        priority: 80,
                        token_estimate: Some(10),
                        freshness: None,
                        confidence: None,
                        content: "retrieved knowledge".into(),
                    },
                ],
                token_breakdown: TokenBreakdown {
                    total: 20,
                    stable_docs: 10,
                    runtime: 0,
                    summaries: 0,
                    fresh_tail: 5,
                    retrieval: 5,
                },
                included_refs: vec![],
                omitted_refs: vec![],
                system_prompt_additions: vec!["workspace_id=test".into()],
            },
            tools: vec![
                ToolDescriptor {
                    name: "read".into(),
                    description: "Read a file".into(),
                    when_to_use: "Use when file contents are needed.".into(),
                    when_not_to_use: "Do not use for destructive actions.".into(),
                    arguments_schema: json!({ "path": "string" }),
                    default_timeout_secs: 5,
                    idempotent: true,
                },
                ToolDescriptor {
                    name: "edit".into(),
                    description: "Edit a file range".into(),
                    when_to_use: "Use when a precise text range must change.".into(),
                    when_not_to_use: "Do not use to create new files.".into(),
                    arguments_schema: json!({ "path": "string", "start_line": "integer", "start_column": "integer", "end_line": "integer", "end_column": "integer", "new_text": "string" }),
                    default_timeout_secs: 5,
                    idempotent: false,
                },
                ToolDescriptor {
                    name: "write".into(),
                    description: "Write a full file".into(),
                    when_to_use: "Use when creating or fully replacing a file.".into(),
                    when_not_to_use: "Do not use for partial edits.".into(),
                    arguments_schema: json!({ "path": "string", "content": "string" }),
                    default_timeout_secs: 5,
                    idempotent: false,
                },
                ToolDescriptor {
                    name: "memory.search".into(),
                    description: "Search long-term memory documents.".into(),
                    when_to_use: "Use for stable facts and preferences.".into(),
                    when_not_to_use: "Do not use when the exact memory entry is already known."
                        .into(),
                    arguments_schema: json!({ "query": "string", "scope": "string" }),
                    default_timeout_secs: 5,
                    idempotent: true,
                },
                ToolDescriptor {
                    name: "memory.get".into(),
                    description: "Read a memory entry by ref.".into(),
                    when_to_use: "Use after memory.search or with a known memory_ref.".into(),
                    when_not_to_use: "Do not use to discover candidates.".into(),
                    arguments_schema: json!({ "memory_ref": "string" }),
                    default_timeout_secs: 5,
                    idempotent: true,
                },
                ToolDescriptor {
                    name: "knowledge.search".into(),
                    description: "Search knowledge libraries.".into(),
                    when_to_use: "Use for evidence-oriented retrieval.".into(),
                    when_not_to_use: "Do not use when the exact document is already known.".into(),
                    arguments_schema: json!({ "query": "string", "library": "string" }),
                    default_timeout_secs: 5,
                    idempotent: true,
                },
                ToolDescriptor {
                    name: "knowledge.get".into(),
                    description: "Read a knowledge document by ref.".into(),
                    when_to_use: "Use after knowledge.search or with a known doc_ref.".into(),
                    when_not_to_use: "Do not use to discover candidates.".into(),
                    arguments_schema: json!({ "doc_ref": "string" }),
                    default_timeout_secs: 5,
                    idempotent: true,
                },
            ],
            conversation_messages: vec![
                SessionMessage::assistant("previous answer"),
                SessionMessage::tool_result("hello"),
                SessionMessage {
                    annotations: vec![SessionMessageAnnotation {
                        kind: "source".into(),
                        value: "user_original".into(),
                    }],
                    ..SessionMessage::user("show me the file")
                },
            ],
            allow_tool_calls: true,
        });

        assert!(xml.contains("<agentjax_prompt version=\"v1\">"));
        assert!(xml.contains("<tool name=\"read\""));
        assert!(xml.contains("<tool name=\"edit\""));
        assert!(xml.contains("<tool name=\"write\""));
        assert!(xml.contains("<tool name=\"memory.search\""));
        assert!(xml.contains("<tool name=\"memory.get\""));
        assert!(xml.contains("<tool name=\"knowledge.search\""));
        assert!(xml.contains("<tool name=\"knowledge.get\""));
        assert!(xml.contains("<message kind=\"assistant\">"));
        assert!(xml.contains("<message kind=\"tool_result\">"));
        assert!(xml.contains("<message kind=\"user\">"));
        assert!(xml.contains("<content>show me the file</content>"));
        assert!(xml.contains("<annotations>"));
    }

    fn identity_pack() -> WorkspaceIdentityPack {
        WorkspaceIdentityPack {
            workspace_id: "workspace.test".into(),
            agent: doc(
                "AGENT.md",
                "## Role\nYou are AgentJax.\n\n## Working Style\nBe direct.",
            ),
            soul: doc("SOUL.md", "## Voice\nCalm."),
            user: doc("USER.md", "## User Profile\nBuilder."),
            memory: doc("MEMORY.md", "## Stable Facts\nPrefers Rust."),
            mission: doc("MISSION.md", "## Mission\nShip runtime."),
            rules: doc("RULES.md", "## Hard Rules\nDo not guess."),
            router: doc("ROUTER.md", "## Tool Use Policy\nUse tools when needed."),
        }
    }

    fn doc(path: &str, content: &str) -> WorkspaceDocument {
        WorkspaceDocument {
            path: path.into(),
            content: content.into(),
        }
    }
}
