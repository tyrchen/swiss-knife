# Instructions

## AWS S3 upload tool

I need a tool to upload files to AWS S3, it shall use the .env file to get the AWS info (e.g. region, profile, s3 bucket name, s3 target path, etc.) then user should be able to:

```bash
s3upload .  # this upload all files to s3
s3upload ./test.mp4 # this upload the test.mp4 file to s3
```

It will check if the destination files exists, and see if local file and remote file are the same, if not, it will upload the file to the S3 bucket. Once the file is uploaded, it will generate a s3 pre-signed url that will be valid for 7 days.

if user provides "--url-only" flag, it will only generate the s3 pre-signed url and print it to the console, without uploading the file to the S3 bucket.

```bash
s3upload ./test.mp4 --url-only # generate s3 pre-signed url for this file only
s3upload . --url-only # generate s3 pre-signed url for all files in the current directory if they have remote files. Give warnings for those haven't been uploaded.
```

Build the tool using Rust with dotenv, clap, and aws sdk. Make UI pretty and user friendly (with progress bar, etc.). First generate a design and implementation plan at ./specs/, then implement the tool entirely.
