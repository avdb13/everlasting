use std::{
    fs::{create_dir, create_dir_all, File},
    path::Path,
    slice,
};

use ahash::{HashSet, HashSetExt};
use color_eyre::Report;
use tracing::debug;

use crate::data::Layout;

pub fn create_layout(layout: Layout) -> Result<(), Report> {
    let full_path = Path::new("./downloads").join(layout.0);
    let _ = create_dir_all(full_path.clone());

    if let Some(files) = layout.1 {
        // figure out why some entries are empty
        let files = files.into_iter().filter(|x| !x.is_empty());

        for file in files {
            if file.len() == 1 {
                File::create(full_path.join(file[0].clone()))?;
            } else {
                let path = file[..file.len() - 1]
                    .iter()
                    .map(Path::new)
                    .map(ToOwned::to_owned)
                    .reduce(|x, acc| x.join(acc))
                    .unwrap();

                let path = full_path.join(path);
                let file = file.last().unwrap();

                let _ = create_dir_all(path.clone());
                File::create(path.join(file))?;
            }
        }
    }

    Ok(())
}
