project_name := `basename "$(pwd)"`
target_machine := "aarch64-unknown-none"

qemu:
    cargo build --target {{target_machine}} --release
    qemu-system-aarch64 \
          -machine virt \
          -cpu cortex-a57 \
          -m 256M \
          -nographic \
          -kernel target/{{target_machine}}/release/{{project_name}}
    
