use crate::base64_plantuml::Base64PlantUML;
use crate::plantuml_backend::PlantUMLBackend;
use deflate::deflate_bytes;
use failure::Error;
use reqwest;
use reqwest::Url;
use std::fs;
use std::io::prelude::*;
use std::path::PathBuf;

/// Helper trait for unit testing purposes (allow testing without a live server)
trait ImageDownloader {
    fn download_image(&self, request_url: &Url) -> Result<Vec<u8>, Error>;
}

struct RealImageDownloader;

impl ImageDownloader for RealImageDownloader {
    /// Download the image at the given URL, return the response body as a
    /// Vec<u8>
    fn download_image(&self, request_url: &Url) -> Result<Vec<u8>, Error> {
        let mut image_buf: Vec<u8> = vec![];
        reqwest::blocking::get(request_url.clone())
            .and_then(|mut response| response.copy_to(&mut image_buf))
            .and_then(|_| Ok(image_buf))
            .or_else(|e| bail!(format!("Failed to generate diagram ({})", e)))
    }
}

pub struct PlantUMLServer {
    server_url: Url,
}

impl PlantUMLServer {
    pub fn new(server_url: Url) -> PlantUMLServer {
        // Make sure the server_url path ends with a / so Url::join works as expected later.
        let path = server_url.path();
        let server_url = if path.ends_with("/") {
            server_url
        } else {
            let mut repath = server_url.clone();
            repath.set_path(format!("{}/", path).as_str());
            repath
        };

        PlantUMLServer {
            server_url: server_url,
        }
    }

    /// Format the PlantUML server URL using the encoded diagram and extension
    fn get_url(&self, image_format: &String, encoded_diagram: &String) -> Result<Url, Error> {
        let path = format!("{}/{}", image_format, encoded_diagram);
        match self.server_url.join(path.as_str()) {
            Ok(url) => Ok(url),
            Err(e) => bail!(format!(
                "Error constructing PlantUML server URL from '{}' and '{}' ({})",
                self.server_url.as_str(),
                path,
                e
            )),
        }
    }

    /// Save the downloaded image to a file
    fn save_downloaded_image(
        &self,
        image_buffer: &Vec<u8>,
        file_path: &PathBuf,
    ) -> Result<(), Error> {
        let mut output_file = fs::File::create(&file_path)?;
        output_file.write_all(&image_buffer)?;

        Ok(())
    }

    /// The business end of this struct, generate the image using the server and
    /// return the relative image URL.
    fn render_string(
        &self,
        plantuml_code: &String,
        output_file: &PathBuf,
        image_format: &String,
        downloader: &dyn ImageDownloader,
    ) -> Result<(), Error> {
        let encoded = encode_diagram_source(plantuml_code);
        let request_url = self.get_url(image_format, &encoded)?;
        let image_buffer = downloader.download_image(&request_url)?;
        self.save_downloaded_image(&image_buffer, &output_file)?;

        Ok(())
    }
}

/// Compress and encode the image source, return the encoed Base64-ish string
fn encode_diagram_source(plantuml_code: &String) -> String {
    let compressed = deflate_bytes(&plantuml_code.as_bytes());
    let base64_compressed = Base64PlantUML::encode(&compressed);

    base64_compressed
}

impl PlantUMLBackend for PlantUMLServer {
    fn render_from_string(
        &self,
        plantuml_code: &String,
        image_format: &String,
        output_file: &PathBuf,
    ) -> Result<(), Error> {
        let downloader = RealImageDownloader {};
        self.render_string(plantuml_code, output_file, image_format, &downloader)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::join_path;
    use pretty_assertions::assert_eq;
    use simulacrum::*;
    use tempfile::tempdir;

    #[test]
    fn test_get_url() {
        let srv = PlantUMLServer::new(Url::parse("http://froboz:1234/plantuml").unwrap());

        assert_eq!(
            Url::parse("http://froboz:1234/plantuml/ext/plantuml_encoded_string").unwrap(),
            srv.get_url(
                &String::from("ext"),
                &String::from("plantuml_encoded_string")
            )
            .unwrap()
        );

        // I cannot manage Url::parse to fail using the ext and encoded data
        // parts :-(. It automatically encodes the invalid characters in the url
        // when parsing. So no test for the error case.
    }

    #[test]
    fn test_get_url_no_path() {
        let srv = PlantUMLServer::new(Url::parse("http://froboz:1234").unwrap());

        assert_eq!(
            Url::parse("http://froboz:1234/ext/plantuml_encoded_string").unwrap(),
            srv.get_url(
                &String::from("ext"),
                &String::from("plantuml_encoded_string")
            )
            .unwrap()
        );
    }

    #[test]
    fn test_encode_diagram_source() {
        assert_eq!(
            String::from("SrRGrQsnKt010000"),
            encode_diagram_source(&String::from("C --|> D"))
        )
    }

    #[test]
    fn test_save_downloaded_image() {
        let tmp_dir = tempdir().unwrap();
        let srv = PlantUMLServer::new(Url::parse("http://froboz").unwrap());

        let data: Vec<u8> = b"totemizer".iter().cloned().collect();
        let img_path = join_path(tmp_dir.path().to_path_buf(), "somefile.ext");
        srv.save_downloaded_image(&data, &img_path).unwrap();

        let raw_source = fs::read(img_path).unwrap();
        assert_eq!("totemizer", String::from_utf8_lossy(&raw_source));
    }

    create_mock! {
        impl ImageDownloader for ImageDownloaderMock (self) {
            expect_download_image("download_image"):
                fn download_image(&self, request_url: &Url) -> Result<Vec<u8>, Error>;
        }
    }

    #[test]
    fn test_render_string() {
        let tmp_dir = tempdir().unwrap();
        let output_path = tmp_dir.into_path();
        let srv = PlantUMLServer::new(Url::parse("http://froboz").unwrap());
        let output_file = join_path(output_path, "foobar.svg");

        let mut mock_downloader = ImageDownloaderMock::new();
        mock_downloader
            .expect_download_image()
            .called_once()
            .with(deref(
                Url::parse("http://froboz/svg/SrRGrQsnKt010000").unwrap(),
            ))
            .returning(|_| Ok(b"the rendered image".iter().cloned().collect()));

        srv.render_string(
            &String::from("C --|> D"),
            &output_file,
            &String::from("svg"),
            &mock_downloader,
        )
        .unwrap();

        let raw_source = fs::read(output_file).unwrap();
        assert_eq!("the rendered image", String::from_utf8_lossy(&raw_source));
    }
}
