use anyhow::Context;
use re_utilities_injector as injector;

fn main() -> anyhow::Result<()> {
    let payload_name = "jc3vrs_payload.dll".to_string();
    let file_name = "JustCause3.exe".to_string();

    let payload_path = std::env::current_exe()?
        .parent()
        .context("failed to find launcher executable directory")?
        .join(payload_name);

    let mut processes = injector::get_processes_by_name(&file_name)?;
    if processes.is_empty() {
        return Err(anyhow::anyhow!("No processes found"));
    }
    let (pid, handle) = processes.remove(0);

    println!("Injecting into process with PID {pid}");
    let payload_path = injector::inject(*handle, &payload_path).context("failed to inject")?;

    println!("Running payload");
    let payload_base = injector::get_remote_module_base(pid, &payload_path)
        .context("failed to get payload base")?
        .context("payload base is null")?;
    injector::call_remote_export(
        *handle,
        payload_base,
        "run",
        Some(std::time::Duration::from_secs(10)),
    )
    .context("failed to call payload run")?;

    Ok(())
}
