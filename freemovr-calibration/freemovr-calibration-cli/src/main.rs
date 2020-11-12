#[macro_use]
extern crate log;

use std::path::PathBuf;
use std::io::Write;
use structopt::StructOpt;

const EXR_COMMENT: Option<&str> = Some("Created by freemovr-calibration-cli.");

#[derive(Debug, StructOpt)]
#[structopt(name = "freemovr-calibration")]
enum Opt {
    #[cfg(feature="opencv")]
    #[structopt(name = "with-checkerboards")]
    WithCheckerboards(WithCheckerboards),

    /// Convert a pinhole wizard .yaml file into a FreeMoVR calibration .exr file.
    #[structopt(name = "generate-exr")]
    GenerateExr(GenerateExr),

    /// Convert a multi-display .yaml file and linked files into a FreeMoVR calibration .exr file.
    #[structopt(name = "multi-display-exr")]
    MultiDisplayExr(MultiDisplayExr),

    // TODO implement a command that lists licenses (based on `cargo lichking
    // bundle`).

    /// (Advanced) Convert a pinhole wizard .yaml file into a corresponding points .csv file.
    #[structopt(name = "generate-csv")]
    GenerateCsv(GenerateCsv),

    /// (Advanced) Convert a corresponding points .csv file into a FreeMoVR calibration .exr file.
    #[structopt(name = "csv2exr")]
    Csv2Exr(Csv2Exr),

    /// (Advanced debugging) Convert a display serface .obj file into a corresponding points .csv file.
    #[structopt(name = "debug-obj2csv")]
    DebugObj2Csv(DebugObj2Csv),
}

#[cfg(feature="opencv")]
#[derive(Debug, StructOpt)]
struct WithCheckerboards {
    /// Filename of input yaml file in pinhole wizard schema
    #[structopt(parse(from_os_str), name="INPUT-YAML")]
    input_yaml: PathBuf,
}

#[derive(Debug, StructOpt)]
struct GenerateExr {
    /// Filename of input yaml file in pinhole wizard schema
    #[structopt(parse(from_os_str), name="SINGLE-DISPLAY-YAML")]
    input_yaml: PathBuf,

    /// Numerical precision
    #[structopt(long="epsilon", default_value="1e-10")]
    epsilon: f64,

    /// Draw debug jpeg images
    #[structopt(long="--save-debug-images")]
    save_debug_images: bool,

    /// Show the viewport mask in the debug jpeg images
    #[structopt(long="--show-mask")]
    show_mask: bool,

}

#[derive(Debug, StructOpt)]
struct GenerateCsv {
    /// Filename of input yaml file in pinhole wizard schema
    #[structopt(parse(from_os_str), name="SINGLE-DISPLAY-YAML")]
    input_yaml: PathBuf,

    /// Numerical precision
    #[structopt(long="epsilon", default_value="1e-10")]
    epsilon: f64,
}

#[derive(Debug, StructOpt)]
struct DebugObj2Csv {
    /// Filename of input obj file with display surface model
    #[structopt(parse(from_os_str), name="DISPLAY-SURFACE-OBJ")]
    display_surface_obj: PathBuf,

    /// World coordinates position of camera
    #[structopt(long="cam-x", default_value="-4.0")]
    cam_x: f64,

    /// World coordinates position of camera
    #[structopt(long="cam-y", default_value="4.0")]
    cam_y: f64,

    /// World coordinates position of camera
    #[structopt(long="cam-z", default_value="1.0")]
    cam_z: f64,
}

#[derive(Debug, StructOpt)]
struct Csv2Exr {
    /// Filename of input csv file
    #[structopt(parse(from_os_str), name="CORRESPONDING-POINTS-CSV")]
    corresponding_points_csv: PathBuf,

    /// Draw debug jpeg images
    #[structopt(long="--save-debug-images")]
    save_debug_images: bool,
}

#[derive(Debug, StructOpt)]
struct MultiDisplayExr {
    /// Filename of input yaml file in multi display schema
    #[structopt(parse(from_os_str), name="MULTI-DISPLAY-YAML")]
    input_yaml: PathBuf,

    /// Numerical precision
    #[structopt(long="epsilon", default_value="1e-10")]
    epsilon: f64,
}

#[cfg(feature="opencv")]
fn with_checkerboards(c: WithCheckerboards) -> Result<(),freemovr_calibration::Error> {
    let src_dir = c.input_yaml.parent().expect("cannot get input directory name");
    let fd = std::fs::File::open(&c.input_yaml)?;

    let data = freemovr_calibration::parse_pinhole_yaml(fd, &src_dir)?;
    use freemovr_calibration::pinhole_wizard_yaml_support::PinholeCalib;
    let (width, height) = (data.loaded.width(), data.loaded.height());
    let intrinsics = freemovr_calibration::intrinsics_from_checkerboards(
        data.loaded.checkerboards().unwrap(), width, height)?;
    info!("computed camera intrinsics for display: {:?}", intrinsics);
    unimplemented!(); // TODO: save to EXR
}

fn generate_csv(c: GenerateCsv) -> Result<(),freemovr_calibration::Error> {
    use failure::ResultExt;
    use freemovr_calibration::PinholeCal;

    let src_dir = c.input_yaml.parent().expect("cannot get input directory name");
    let fd = std::fs::File::open(&c.input_yaml)
        .context(format!("opening file: {}",c.input_yaml.display()))?;
    let src_data = freemovr_calibration::ActualFiles::new(fd, &src_dir, c.epsilon)?;
    let trimesh = src_data.geom_as_trimesh().unwrap();

    let pinhole_fits = src_data.pinhole_fits();
    assert!(pinhole_fits.len()==1);
    let (_name, cam) = &pinhole_fits[0];

    let out_fname = "out.csv";
    let mut file = std::fs::File::create(out_fname)?;
    info!("saving CSV output file: {}", out_fname);
    let created_at = Some(chrono::Local::now());
    freemovr_calibration::export_to_csv(&mut file, &cam, &trimesh, created_at)?;
    Ok(())
}

fn debug_obj2csv(c: DebugObj2Csv) -> Result<(),freemovr_calibration::Error> {
    use failure::ResultExt;
    use nalgebra::Vector3;

    // load OBJ file with display surface
    let file = std::fs::File::open(&c.display_surface_obj)
        .context(format!("loading geometry from file: {}",
            c.display_surface_obj.display()))?;
    let trimesh = freemovr_calibration::parse_obj_from_reader(file, c.display_surface_obj.to_str())?;

    let sum_vec = trimesh.worldcoords().points().iter()
        .fold( Vector3::new(0.0, 0.0, 0.0), |acc, v| acc + v.coords );
    let mean_vec = sum_vec/ (trimesh.worldcoords().points().len() as f64);
    let obj_center = mean_vec;

    // create a camera viewing display surface
    let camcenter = Vector3::new( c.cam_x, c.cam_y, c.cam_z);
    let up = nalgebra::core::Unit::new_normalize(Vector3::new( 0.0, 0.0, 1.0));
    let extrinsics = cam_geom::ExtrinsicParameters::from_view(&camcenter, &obj_center, &up);

    let params = cam_geom::PerspectiveParams {
        fx: 100.0,
        fy: 100.0,
        skew: 0.0,
        cx: 512.0,
        cy: 384.0,
    };
    let intrinsics: cam_geom::IntrinsicParametersPerspective<_> = params.into();

    let cam = mvg::Camera::new(1024, 768, extrinsics, intrinsics.into())?;

    let out_fname = "out.csv";
    let mut file = std::fs::File::create(out_fname)?;
    info!("saving CSV output file: {}", out_fname);
    let created_at = Some(chrono::Local::now());
    freemovr_calibration::export_to_csv(&mut file, &cam, &trimesh, created_at)?;
    Ok(())
}

fn csv2exr(c: Csv2Exr) -> Result<(),freemovr_calibration::Error> {
    use failure::ResultExt;
    let out_fname = "out.exr";
    let mut exr_file = std::fs::File::create(out_fname)?;
    let csv_file = std::fs::File::open(&c.corresponding_points_csv).context(
        format!("Could not open point corresponding points csv file: {}",
        c.corresponding_points_csv.display()))?;

    freemovr_calibration::csv2exr(&csv_file, &mut exr_file, c.save_debug_images, EXR_COMMENT)?;
    Ok(())
}

fn no_distortion(c: GenerateExr) -> Result<(),freemovr_calibration::Error> {
    use failure::ResultExt;

    if !c.save_debug_images && c.show_mask {
        error!("cannot show mask unless saving debug images.");
    }

    let src_dir = c.input_yaml.parent().expect("cannot get input directory name");
    let fd = std::fs::File::open(&c.input_yaml)
        .context(format!("opening file: {}",c.input_yaml.display()))?;
    let src_data = freemovr_calibration::ActualFiles::new(fd, &src_dir, c.epsilon)?;
    let float_image = freemovr_calibration::fit_pinholes_compute_cal_image(&src_data,
        c.save_debug_images, c.show_mask)?;
    let out_fname = "out.exr";
    let mut file = std::fs::File::create(out_fname)?;
    let mut exr_writer = freemovr_calibration::ExrWriter::new();
    info!("saving EXR output file: {}", out_fname);
    exr_writer.update(&float_image, EXR_COMMENT);
    file.write(&exr_writer.buffer())?;
    Ok(())
}

fn multi_display(c: MultiDisplayExr) -> Result<(),freemovr_calibration::Error> {
    use failure::ResultExt;

    let src_dir = c.input_yaml.parent().expect("cannot get input directory name");
    let fd = std::fs::File::open(&c.input_yaml)
        .context(format!("opening file: {}",c.input_yaml.display()))?;
    let float_image = freemovr_calibration::do_multi_display(fd, c.epsilon, &src_dir)?;
    let out_fname = "multi.exr";
    let mut file = std::fs::File::create(out_fname)?;
    let mut exr_writer = freemovr_calibration::ExrWriter::new();
    info!("saving EXR output file: {}", out_fname);
    exr_writer.update(&float_image, EXR_COMMENT);
    file.write(&exr_writer.buffer())?;
    Ok(())
}


fn main() -> Result<(),freemovr_calibration::Error> {
    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "freemovr_calibration=info,freemovr_calibration_cli=info,error");
    }

    env_logger::init();
    let opt = Opt::from_args();

    match opt {
        #[cfg(feature="opencv")]
        Opt::WithCheckerboards(c) => with_checkerboards(c),
        Opt::GenerateExr(c) => no_distortion(c),
        Opt::MultiDisplayExr(c) => multi_display(c),

        // advanced
        Opt::DebugObj2Csv(c) => debug_obj2csv(c),
        Opt::GenerateCsv(c) => generate_csv(c),
        Opt::Csv2Exr(c) => csv2exr(c),
    }
}
