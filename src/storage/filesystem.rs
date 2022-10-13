use core::time::Duration;
use std::time::SystemTime;

use actix_web::web::Bytes;
use async_stream::try_stream;
use camino::Utf8Component;
use camino::Utf8Path;
use camino::Utf8PathBuf;
use clap::Parser;
use futures::stream::BoxStream;
use futures::stream::Stream;
use futures::stream::StreamExt;
use tokio::fs::create_dir_all;
use tokio::fs::symlink_metadata;
use tokio::fs::File;
use tokio::fs::OpenOptions;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;
use tokio::io::BufWriter;

#[derive(Clone, Debug, Parser)]
pub struct FilesystemConfig {
	#[clap(env, long)]
	filesystem_root: Utf8PathBuf
}

impl FilesystemConfig {
	pub fn repository(&self) -> Repository {
		Repository{
			root: self.filesystem_root.clone()
		}
	}
}

#[derive(Debug, Clone)]
pub struct Repository {
	root: Utf8PathBuf
}

impl Repository {
	fn full_path(&self, path: &Utf8Path) -> Utf8PathBuf {
		self.root.join(path
			.components()
			.filter(|c| matches!(c, Utf8Component::ParentDir | Utf8Component::Normal(_)))
			.collect::<Utf8PathBuf>()
		)
	}

	pub async fn check_if_exists(&self, object: &Utf8Path) -> Result<bool, std::io::Error> {
		let path = self.full_path(object);
		match symlink_metadata(&path).await {
			Ok(metadata) => Ok(metadata.is_file()),
			Err(e) if(e.kind() == std::io::ErrorKind::NotFound) => Ok(false),
			Err(e) => Err(e)
		}
	}

	pub async fn age(&self, object: &Utf8Path) -> Result<Duration, std::io::Error> {
		let path = self.full_path(object);
		let metadata = symlink_metadata(&path).await?;
		Ok(SystemTime::now().duration_since(metadata.modified()?).unwrap_or_default())
	}

	pub async fn read(self, object: &Utf8Path) -> Result<BoxStream<'static, Result<Bytes, std::io::Error>>, std::io::Error> {
		let path = self.full_path(&object);
		let mut file = BufReader::with_capacity(16384, File::open(path).await?);
		Ok(Box::pin(try_stream! {
			loop {
				let buf = file.fill_buf().await?;
				if(buf.len() == 0) {
					break;
				}
				yield Bytes::copy_from_slice(buf);
			}
		}))
	}

	pub async fn write(&self, object: &Utf8Path, mut reader: impl Stream<Item = Result<&[u8], std::io::Error>> + Unpin) -> Result<(), std::io::Error> {
		let path = self.full_path(object);
		if let Some(parent) = path.parent() {
			create_dir_all(parent).await?;
		}
		let file = OpenOptions::default().create(true).read(false).write(true).truncate(true).open(&path).await?;
		let mut file = BufWriter::with_capacity(16384, file);
		while let Some(buf) = reader.next().await {
			let buf = buf?;
			if(buf.len() == 0) {
				break;
			}
			file.write_all(buf).await?;
		}
		file.flush().await?;
		Ok(())
	}
}
