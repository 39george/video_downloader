pub mod interceptor;
mod run;
pub mod video_saver;

/// This macro is for tracing error and returning Result if there are some
/// meaningful Ok() case, and returning () if there are no meaningful result.
/// It is useful to simply trace error message on fallible operations which doesn't
/// return anything in the Ok() branch.
#[macro_export]
macro_rules! print_err {
    ($exp:expr) => {
        match $exp {
            Ok(v) => Ok(v),
            Err(e) => {
                println!("{e}");
                Err(e)
            }
        }
    };
    ($exp:expr, ()) => {
        match $exp {
            Ok(()) => (),
            Err(e) => {
                println!("{e}");
                ()
            }
        }
    };
}

fn main() {
    run::run().unwrap();
}
