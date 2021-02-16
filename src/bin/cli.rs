use clap::{App, Arg};
use ingredient::ingredient;

pub fn main() {
    let matches = App::new("MyApp")
        .arg(Arg::from_usage(
            "-c --config=[CONFIG] 'Optionally sets a config file to use'",
        ))
        .get_matches();

    // We can find out whether or not "config" was used
    if matches.is_present("config") {
        println!("An config file was specified");
    }

    // We can also get the value for "config"
    //
    // NOTE: If we specified multiple(), this will only return the _FIRST_
    // occurrence
    if let Some(ref in_file) = matches.value_of("config") {
        println!("An config file: {}", in_file);
        match ingredient(in_file, true) {
            Ok(i) => println!("{}", i),
            Err(e) => println!("fail: {}", e),
        }
    }
}
