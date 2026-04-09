#[tokio::main]
async fn main() -> anyhow::Result<()> {
    agentjax::cli::run().await
}
