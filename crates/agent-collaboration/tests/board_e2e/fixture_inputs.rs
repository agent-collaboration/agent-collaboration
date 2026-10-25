//! Shared literal setup for the board process journeys, always through public entry points.
use super::fixture::{BoardFixture, accepted, at, fields, header, identity};
use codex_router_test_support::FixtureResult;
use serde_json::{Value, json};

impl BoardFixture {
    pub async fn create_workspace_board(&self, workspace: &str, board: &str) -> FixtureResult {
        self.accept(
            "workspace_create",
            fields(
                header(&self.owner),
                json!({"workspaceId":workspace,
            "name":workspace,"description":"Owned synthetic fixture"}),
            )?,
        )
        .await?;
        self.accept("board_create",fields(header(&self.owner),json!({"workspaceId":workspace,
            "boardId":board,"name":board,"description":"Owned synthetic fixture","kind":{"kind":"area"}}))?).await?;
        Ok(())
    }

    pub async fn create_project(&self, board: &str, project: &str, title: &str) -> FixtureResult {
        let actor = self.owner.to_string();
        let request = identity();
        let arguments = [
            "project",
            "create",
            "--board-id",
            board,
            "--project-id",
            project,
            "--title",
            title,
            "--request-id",
            &request,
            "--actor",
            &actor,
        ];
        accepted(&self.cli_write(&arguments).await?)?;
        self.accept(
            "role_grant",
            json!({"requestId":identity(),"protocolVersion":1,"actingFor":null,
            "actor":self.owner,"projectId":project,"operation":{"kind":"roleGrant",
            "subject":{"kind":"project","subject":project},"role":"lead","holder":self.owner,
            "model":null,"lineage":null,"expectedRevision":0,"ownerGrant":null}}),
        )
        .await?;
        Ok(())
    }

    pub async fn create(&self) -> FixtureResult {
        self.create_workspace_board(&self.workspace, &self.board)
            .await?;
        self.create_project(&self.board, &self.project, "Rust journey project")
            .await?;
        let reply = self
            .cli(&[
                "workstream",
                "create",
                "--project-id",
                &self.project,
                "--workstream-id",
                &self.workstream,
                "--name",
                "verification lane",
                "--goal",
                "observe recorded outcomes",
                "--actor",
                &self.owner.to_string(),
            ])
            .await?;
        accepted(at(&reply, "/record")?)?;
        Ok(())
    }

    pub async fn foreign_project(&self) -> FixtureResult<(String, String)> {
        let workspace = identity();
        let board = identity();
        let project = identity();
        self.create_workspace_board(&workspace, &board).await?;
        self.create_project(&board, &project, &format!("foreign {project}"))
            .await?;
        Ok((workspace, project))
    }

    pub async fn thread(&self, project: &str, actor: &Value) -> FixtureResult<String> {
        let thread = identity();
        let text = self.root.path().join(format!("root-{thread}.txt"));
        std::fs::write(&text, format!("root {thread}"))?;
        let reply = self
            .cli(&[
                "thread",
                "create",
                "--project-id",
                project,
                "--post-id",
                &thread,
                "--about",
                &format!("project:verification thread {thread}"),
                "--actor",
                &actor.to_string(),
                "--no-watch",
                "--text-file",
                text.to_str().ok_or("root path")?,
            ])
            .await?;
        accepted(at(&reply, "/record")?)?;
        Ok(thread)
    }

    pub fn reply_request(&self, project: &str, thread: &str, text: &str) -> FixtureResult<Value> {
        fields(
            header(&self.owner),
            json!({"postId":identity(),"placement":{"kind":"thread",
            "thread":{"project":project,"thread":thread}},"text":text,"references":[],"addressees":[]}),
        )
    }

    pub async fn reply(&self, project: &str, thread: &str, text: &str) -> FixtureResult<Value> {
        let request = self.reply_request(project, thread, text)?;
        let post = at(&request, "/postId")?.as_str().ok_or("post ID")?;
        let id = at(&request, "/requestId")?.as_str().ok_or("request ID")?;
        let reply = self
            .cli(&[
                "post",
                "post",
                "--placement",
                "thread",
                "--thread-id",
                &format!("{project}/{thread}"),
                "--post-id",
                post,
                "--request-id",
                id,
                "--text",
                text,
                "--actor",
                &self.owner.to_string(),
            ])
            .await?;
        accepted(at(&reply, "/record")?)?;
        Ok(request)
    }

    pub async fn initialize(&self, reader: &Value, project: &str) -> FixtureResult {
        let reply = self
            .cli(&[
                "inbox",
                "fetch",
                "--scope",
                "project",
                "--project-id",
                project,
                "--reader",
                &reader.to_string(),
            ])
            .await?;
        assert_eq!(reply.pointer("/page/records"), Some(&json!([])));
        Ok(())
    }

    pub async fn inbox(&self, reader: &Value, additions: Value) -> FixtureResult<Value> {
        self.host()?
            .mcp(
                "inbox_fetch",
                fields(
                    json!({"protocolVersion":1,"actingFor":null,"reader":reader,
            "scope":{"kind":"workspace","workspaceId":self.workspace},"readMode":"unread",
            "initializations":[],"page":{"limit":100}}),
                    additions,
                )?,
            )
            .await
    }
}
