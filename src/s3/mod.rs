pub mod client;
pub mod compare;
pub mod error;
pub mod multipart;
pub mod presign;
pub mod upload;

pub use client::S3Client;
pub use compare::FileComparison;
pub use error::S3UploadError;
pub use multipart::{upload_multipart, MULTIPART_THRESHOLD};
pub use presign::generate_presigned_url;
pub use upload::{upload_file, UploadResult};

// Re-export Result for internal use
#[allow(unused_imports)]
pub(crate) use error::Result;
