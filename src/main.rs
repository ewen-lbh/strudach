use std::{path::PathBuf, str::FromStr};

use docopt::Docopt;

// TODO: allow stdin
// Usage: strudach [options] (<schema> | -) <input>...
//        strudach [options] convert <schema> [<output>]

const USAGE: &'static str = "
Usage: strudach [options] <schema> <input>...
       strudach [options] convert <schema> <output>

Options:
    -h, --help          Show this message.
    -v, --version       Show version.
";

fn main() {
    let args = Docopt::new(USAGE)
        .and_then(|d| d.argv(std::env::args()).parse())
        .unwrap_or_else(|e| e.exit());

    let schema_file = PathBuf::from_str(args.get_str("<schema>"));
    let schema = match strudach::load(schema_file) {
        Some(s) => s,
        Err(e) => {
            println!("Error loading schema: {}", e);
            return;
        }
    };

    if args.get_bool("convert") {
        let output_file = args.get_str("<output>");
        println!("TODO")
    } else {
        let input_files = args.get_vec("<input>").map(PathBuf::from_str);
        let validation_errors = match strudach::validate(schema, input_files) {
            Ok(errors) => errors,
            Err(e) => {
                println!("Error while validating: {}", e);
                return;
            }
        };
        for validation_error in validation_errors {
            println!(
                "in {}:{} - {}",
                validation_error.file,
                validation_error.path.join("."),
                validation_error.message
            );
        }
    }
}
