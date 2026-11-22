use anyhow::{Context, Result, bail};
use kapsule::{RunOptions, check, run};
use std::path::PathBuf;

const HELP: &str = "Kapsule — lecteur autonome d’applications Lua 5.4\n\nUsage: kapsule [OPTIONS] DOSSIER_OU_APPS.TOML [-- ARGUMENTS...]\n\n  --windowed          Utiliser les dimensions du manifeste (F5 : zoom)\n  --check             Vérifier le manifeste et la syntaxe sans exécuter Lua\n  --headless FRAMES   Exécuter sans terminal, à pas de temps fixe\n  --dump-screen       Afficher le dernier écran en mode headless\n  --cwd DOSSIER       Répertoire de lancement pour kkc et kkc-shell\n  -h, --help          Afficher cette aide\n  -V, --version       Afficher la version\n\nCtrl-C quitte le lecteur. Esc est transmis à la Kapsule.\n";
fn main() {
    if let Err(error) = execute() {
        eprintln!("kapsule: {error:#}");
        std::process::exit(1);
    }
}
fn execute() -> Result<()> {
    let mut options = RunOptions::default();
    let mut check_only = false;
    let mut path = None;
    let mut cwd = None;
    let mut app_args = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print!("{HELP}");
                return Ok(());
            }
            "-V" | "--version" => {
                println!("kapsule {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "--windowed" => options.windowed = true,
            "--check" => check_only = true,
            "--dump-screen" => options.dump_screen = true,
            "--headless" => {
                let frames = args
                    .next()
                    .context("--headless requires a positive frame count")?
                    .parse::<u32>()?;
                anyhow::ensure!(frames > 0, "--headless requires a positive frame count");
                options.headless_frames = Some(frames);
            }
            "--cwd" => {
                let dir = PathBuf::from(args.next().context("--cwd requires a directory")?)
                    .canonicalize()?;
                anyhow::ensure!(dir.is_dir(), "--cwd must be a directory");
                cwd = Some(dir);
            }
            "--" => {
                app_args.extend(args);
                break;
            }
            _ if arg.starts_with('-') => bail!("Unknown option {arg}; use --help"),
            _ if path.is_none() => path = Some(PathBuf::from(arg)),
            _ => {
                app_args.push(arg);
                app_args.extend(args);
                break;
            }
        }
    }
    let Some(path) = path else {
        print!("{HELP}");
        return Ok(());
    };
    anyhow::ensure!(
        !options.dump_screen || options.headless_frames.is_some(),
        "--dump-screen requires --headless"
    );
    if check_only {
        check(&path)?;
        println!("Kapsule valide : {}", path.display());
        Ok(())
    } else {
        run(&path, &app_args, cwd.as_deref(), &options)
    }
}
