use agentjax::bootstrap::bootstrap_application;

fn main() -> anyhow::Result<()> {
    let _app = bootstrap_application()?;
    Ok(())
}
