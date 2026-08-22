use clap::Parser;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Cli {
    /// Edit files in place. With an attached extension (`-i=.bak`) the original
    /// is backed up first. The extension uses `=` so the program argument is
    /// never mistaken for the backup suffix.
    #[arg(short = 'i', num_args = 0..=1, require_equals = true, default_missing_value = "")]
    pub i: Option<Option<String>>,

    /// Accepted for sed compatibility; se only prints when the program asks it to
    #[arg(short, default_value_t = false)]
    pub n: bool,

    /// Extended regular expressions (enabled by default in se)
    #[arg(short = 'E', default_value_t = true)]
    pub extended_regexp: bool,

    /// Watch a file and re-run the program when it changes (macOS only).
    #[arg(short = 'w', default_value_t = false)]
    pub watch: bool,

    /// Pipeline script to execute (e.g. "x/error/ p"). A program that *begins*
    /// with the `-` collapse operator must be preceded by `--` so clap does not
    /// read it as an option, e.g. `se -- '- p' file`.
    #[arg(name = "PROGRAM")]
    pub program: String,

    /// Input file paths
    #[arg(name = "FILE", required = false)]
    pub files: Vec<String>,
}
