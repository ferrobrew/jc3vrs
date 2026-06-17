# Native Windows build: compile the payload + injector and run the injector.
# The injector loads jc3vrs_payload.dll from its own directory.
cargo build --target x86_64-pc-windows-msvc -p jc3vrs_payload
if (-not $?) {
    Write-Error "Failed to build jc3vrs_payload"
    exit 1
}

cargo build --target x86_64-pc-windows-msvc -p jc3vrs_injector
if (-not $?) {
    Write-Error "Failed to build jc3vrs_injector"
    exit 1
}

& "./target/x86_64-pc-windows-msvc/debug/jc3vrs_injector.exe" @args
