use std::process::Command;

fn main() {
    {
        Command::new("gcc")
            .args(&["tests/zombie.c", "-o", "tests/zombie"])
            .status()
            .unwrap();
    }
}
