use clap::{App, Arg};
use ingredient::from_str;

pub fn main() {
    let matches = App::new("MyApp")
        .arg(Arg::from_usage(
            "-c --config=[CONFIG] 'Optionally sets a config file to use'",
        ))
        .get_matches();

    if let Some(ref in_file) = matches.value_of("config") {
        println!("IN: {}", in_file);
        match from_str(in_file, true) {
            Ok(i) => println!("OUT: {}", i),
            Err(e) => println!("fail: {}", e),
        }
    }
}

#[cfg(test)]
mod tests {

    use assert_cmd::prelude::*; // Add methods on commands
    use predicates::prelude::*; // Used for writing assertions

    use std::process::Command; // Run programs
    #[test]
    fn test_cli() -> Result<(), Box<dyn std::error::Error>> {
        let mut cmd = Command::cargo_bin("cli")?;

        cmd.arg("-c").arg("1g potato");
        cmd.assert()
            .success()
            .stdout(predicate::str::contains("IN: 1g potato\nOUT: 1 g potato"));

        Ok(())
    }
}
