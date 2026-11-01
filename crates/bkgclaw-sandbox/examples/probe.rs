//! One question, one answer: can this kernel apply a Landlock ruleset?
//!
//! Used interactively while wiring the compose deployment — the jail refuses
//! every cycle when the answer is no, and the sysfs node
//! (`/proc/sys/kernel/security/landlock/features/abi`) is absent on hosts
//! where securityfs is simply not mounted, which says nothing about the
//! syscall. The probe is the syscall, so the probe is the truth.
//!
//! ```sh
//! cargo run -p bkgclaw-sandbox --example probe
//! ```

fn main() {
    let doctor = bkgclaw_sandbox::jail::doctor();
    println!("landlock_abi: {:?}", doctor.landlock_abi);
    println!("can_jail:    {}", doctor.can_jail);
}
