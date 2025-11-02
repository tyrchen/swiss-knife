pub mod client;
pub mod compare;
pub mod error;
pub mod helpers;
pub mod multipart;
pub mod presign;
pub mod upload;

pub use client::S3Client;
pub use compare::FileComparison;
pub use helpers::{detect_content_type, parse_metadata, parse_tags};
pub use multipart::{upload_multipart, MULTIPART_THRESHOLD};
pub use presign::{generate_presigned_url, generate_presigned_url_with_expiry};
pub use upload::{upload_file, UploadResult};

// Re-export error types for potential future use
#[allow(unused_imports)]
pub use error::S3UploadError;

// Re-export Result for internal use
#[allow(unused_imports)]
pub(crate) use error::Result;
