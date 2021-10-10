use anyhow::Context;
use std::path::PathBuf;
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(name = "braidz-cli")]
struct Opt {
    /// Input braidz filename
    #[structopt(parse(from_os_str))]
    input: PathBuf,

    /// print all data in the `data2d_distorted` table
    #[structopt(short, long)]
    data2d_distorted: bool,
}

fn main() -> anyhow::Result<()> {
    env_logger::init();
    let opt = Opt::from_args();
    let attr = std::fs::metadata(&opt.input)
        .with_context(|| format!("Getting file metadata for {}", opt.input.display()))?;

    let mut archive = braidz_parser::braidz_parse_path(&opt.input)
        .with_context(|| format!("Parsing file {}", opt.input.display()))?;

    let summary =
        braidz_parser::summarize_braidz(&archive, opt.input.display().to_string(), attr.len());

    let yaml_buf = serde_yaml::to_string(&summary)?;
    println!("{}", yaml_buf);

    if opt.data2d_distorted {
        println!("data2d_distorted table: --------------");
        for row in archive.iter_data2d_distorted()? {
            println!("{:?}", row);
        }
    }

    Ok(())
}
