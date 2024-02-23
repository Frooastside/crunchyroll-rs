use crate::error::Error;
use crate::media::Stream;
use crate::{Executor, Locale, Request, Result};
use serde::{Deserialize, Serialize};
use std::borrow::BorrowMut;
use std::fmt::Formatter;
use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

/// Segment decryption key.
#[cfg(feature = "hls-stream")]
#[cfg_attr(docsrs, doc(cfg(any(feature = "hls-stream", feature = "dash-stream"))))]
pub type Aes128CbcDec = cbc::Decryptor<aes::Aes128>;
/// Segment decryption key.
#[cfg(not(feature = "hls-stream"))]
#[cfg_attr(docsrs, doc(cfg(any(feature = "hls-stream", feature = "dash-stream"))))]
pub type Aes128CbcDec = ();

impl Stream {
    /// Returns streaming data which can be used to get the literal stream data and
    /// process it further (e.g. write them to a file which than can be played), based
    /// of the [HLS](https://en.wikipedia.org/wiki/HTTP_Live_Streaming) stream
    /// Crunchyroll provides.
    /// The locale argument specifies which hardsub (subtitles which are "burned" into
    /// the video) the returned data should have. You can get a list of supported locales
    /// by calling [`Stream::streaming_hardsub_locales`].
    /// The result contains video + audio data (combined). If you want to get video and
    /// audio separately, check out [`Stream::dash_streaming_data`].
    /// Note that this is only the implementation of this crate to stream data. You can
    /// still manually use the variants in [`Stream::variants`] and implement the streaming on
    /// your own.
    /// If this function fails with [`Error::Input`] `no stream available`, this probably means
    /// that no non-drm endpoint is available.
    #[cfg(feature = "hls-stream")]
    #[cfg_attr(docsrs, doc(cfg(feature = "hls-stream")))]
    pub async fn hls_streaming_data(&self, hardsub: Option<Locale>) -> Result<Vec<VariantData>> {
        if let Some(locale) = hardsub {
            if let Some(raw_streams) = self.variants.get(&locale) {
                VariantData::from_hls_master(
                    self.executor.clone(),
                    raw_streams
                        .adaptive_hls
                        .as_ref()
                        .ok_or(Error::Input {
                            message: "no stream available".to_string(),
                        })?
                        .url
                        .clone(),
                )
                .await
            } else {
                Err(Error::Input {
                    message: format!("could not find any stream with hardsub locale '{}'", locale),
                })
            }
        } else if let Some(raw_streams) = self.variants.get(&Locale::Custom("".into())) {
            VariantData::from_hls_master(
                self.executor.clone(),
                raw_streams
                    .adaptive_hls
                    .as_ref()
                    .ok_or(Error::Input {
                        message: "no stream available".to_string(),
                    })?
                    .url
                    .clone(),
            )
            .await
        } else if let Some(raw_streams) = self.variants.get(&Locale::Custom(":".into())) {
            VariantData::from_hls_master(
                self.executor.clone(),
                raw_streams
                    .adaptive_hls
                    .as_ref()
                    .ok_or(Error::Input {
                        message: "no stream available".to_string(),
                    })?
                    .url
                    .clone(),
            )
            .await
        } else {
            Err(Error::Internal {
                message: "could not find supported stream".to_string(),
            })
        }
    }

    /// Returns streaming data which can be used to get the literal stream data and
    /// process it further (e.g. write them to a file which than can be played), based
    /// of the
    /// [MPEG-DASH](https://en.wikipedia.org/wiki/Dynamic_Adaptive_Streaming_over_HTTP)
    /// stream Crunchyroll provides.
    /// The locale argument specifies which hardsub (subtitles which are "burned" into
    /// the video) the returned data should have. You can get a list of supported locales
    /// by calling [`Stream::streaming_hardsub_locales`].
    /// The result is a tuple; the first [`Vec<VariantData>`] contains only video data,
    /// without any audio; the second [`Vec<VariantData>`] contains only audio data,
    /// without any video. If you want video + audio combined, check out
    /// [`Stream::dash_streaming_data`].
    /// Note that this is only the implementation of this crate to stream data. You can
    /// still manually use the variants in [`Stream::variants`] and implement the streaming on
    /// your own.
    /// If this function fails with [`Error::Input`] `no stream available`, this probably means
    /// that no non-drm endpoint is available.
    #[cfg(feature = "dash-stream")]
    #[cfg_attr(docsrs, doc(cfg(feature = "dash-stream")))]
    pub async fn dash_streaming_data(
        &self,
        hardsub: Option<Locale>,
    ) -> Result<(Vec<VariantData>, Vec<VariantData>)> {
        let url = if let Some(locale) = hardsub {
            if let Some(raw_streams) = self.variants.get(&locale) {
                raw_streams
                    .adaptive_dash
                    .as_ref()
                    .ok_or(Error::Input {
                        message: "no stream available".to_string(),
                    })?
                    .url
                    .clone()
            } else {
                return Err(Error::Input {
                    message: format!("could not find any stream with hardsub locale '{}'", locale),
                });
            }
        } else if let Some(raw_streams) = self.variants.get(&Locale::Custom("".into())) {
            raw_streams
                .adaptive_dash
                .as_ref()
                .ok_or(Error::Input {
                    message: "no stream available".to_string(),
                })?
                .url
                .clone()
        } else {
            return Err(Error::Internal {
                message: "could not find supported stream".to_string(),
            });
        };

        let mut video = vec![];
        let mut audio = vec![];

        let raw_mpd = self.executor.get(&url).request_raw().await?;
        let period = dash_mpd::parse(
            String::from_utf8_lossy(raw_mpd.as_slice())
                .to_string()
                .as_str(),
        )
        .map_err(|e| Error::Decode {
            message: e.to_string(),
            content: raw_mpd,
            url,
        })?
        .periods[0]
            .clone();
        let adaptions = period.adaptations;

        for adaption in adaptions {
            if adaption.maxWidth.is_some() || adaption.maxHeight.is_some() {
                video.extend(
                    VariantData::from_mpeg_mpd_representations(
                        self.executor.clone(),
                        adaption.SegmentTemplate.expect("dash segment template"),
                        adaption.representations,
                    )
                    .await?,
                )
            } else {
                audio.extend(
                    VariantData::from_mpeg_mpd_representations(
                        self.executor.clone(),
                        adaption.SegmentTemplate.expect("dash segment template"),
                        adaption.representations,
                    )
                    .await?,
                )
            }
        }

        Ok((video, audio))
    }

    /// Return all supported hardsub locales which can be used as argument in
    /// [`Stream::hls_streaming_data`].
    pub fn streaming_hardsub_locales(&self) -> Vec<Locale> {
        self.variants.keys().cloned().collect::<Vec<Locale>>()
    }
}

/// Video resolution.
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Resolution {
    pub width: u64,
    pub height: u64,
}

impl std::fmt::Display for Resolution {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}x{}", self.width, self.height)
    }
}

impl From<m3u8_rs::Resolution> for Resolution {
    fn from(resolution: m3u8_rs::Resolution) -> Self {
        Self {
            height: resolution.height,
            width: resolution.width,
        }
    }
}

#[derive(Clone, Debug)]
enum VariantDataUrl {
    #[cfg(feature = "hls-stream")]
    Hls { url: String },
    #[cfg(feature = "dash-stream")]
    MpegDash {
        id: String,
        base: String,
        init: String,
        fragments: String,
        start: u32,
        /// Length of each segment as milliseconds. The length if this field is also the number of
        /// segments.
        lengths: Vec<u32>,
    },
}

/// Streaming data for a variant.
#[allow(dead_code)]
#[derive(Serialize, Clone, Debug, Request)]
#[request(executor(segments))]
pub struct VariantData {
    #[serde(skip)]
    executor: Arc<Executor>,

    pub resolution: Resolution,
    pub bandwidth: u64,
    pub fps: f64,
    pub codecs: String,

    #[serde(skip)]
    url: VariantDataUrl,
}

impl VariantData {
    #[cfg(feature = "hls-stream")]
    async fn from_hls_master(executor: Arc<Executor>, url: String) -> Result<Vec<VariantData>> {
        let raw_master_playlist = executor.get(&url).request_raw().await?;

        let master_playlist = m3u8_rs::parse_master_playlist_res(raw_master_playlist.as_slice())
            .map_err(|e| Error::Decode {
                message: e.to_string(),
                content: raw_master_playlist.clone(),
                url,
            })?;

        let mut stream_data: Vec<VariantData> = vec![];

        for variant in master_playlist.variants {
            #[cfg(not(feature = "__test_strict"))]
            stream_data.push(VariantData {
                executor: executor.clone(),

                resolution: variant
                    .resolution
                    .unwrap_or(m3u8_rs::Resolution {
                        height: 0,
                        width: 0,
                    })
                    .into(),
                bandwidth: variant.bandwidth,
                fps: variant.frame_rate.unwrap_or_default(),
                codecs: variant.codecs.unwrap_or_default(),

                url: VariantDataUrl::Hls { url: variant.uri },
            });

            #[cfg(feature = "__test_strict")]
            stream_data.push(VariantData {
                executor: executor.clone(),

                resolution: variant.resolution.unwrap().into(),
                bandwidth: variant.bandwidth,
                fps: variant.frame_rate.unwrap(),
                codecs: variant.codecs.unwrap(),

                url: VariantDataUrl::Hls { url: variant.uri },
            });
        }

        Ok(stream_data)
    }

    #[cfg(feature = "dash-stream")]
    async fn from_mpeg_mpd_representations(
        executor: Arc<Executor>,
        segment_template: dash_mpd::SegmentTemplate,
        representations: Vec<dash_mpd::Representation>,
    ) -> Result<Vec<VariantData>> {
        let mut stream_data = vec![];

        for representation in representations {
            let string_fps = representation.frameRate.unwrap_or_default();

            let fps = if let Some((l, r)) = string_fps.split_once('/') {
                let left = l.parse().unwrap_or(0f64);
                let right = r.parse().unwrap_or(0f64);
                if left != 0f64 && right != 0f64 {
                    left / right
                } else {
                    0f64
                }
            } else {
                string_fps.parse().unwrap_or(0f64)
            };

            #[cfg(not(feature = "__test_strict"))]
            stream_data.push(VariantData {
                executor: executor.clone(),
                resolution: Resolution {
                    height: representation.height.unwrap_or_default(),
                    width: representation.width.unwrap_or_default(),
                },
                bandwidth: representation.bandwidth.unwrap_or_default(),
                fps,
                codecs: representation.codecs.unwrap_or_default(),
                url: VariantDataUrl::MpegDash {
                    id: representation.id.expect("dash representation id"),
                    base: representation
                        .BaseURL
                        .get(0)
                        .expect("dash base url")
                        .base
                        .clone(),
                    init: segment_template
                        .initialization
                        .clone()
                        .expect("dash initialization url"),
                    fragments: segment_template.media.clone().expect("dash media url"),
                    start: segment_template.startNumber.expect("dash start number") as u32,
                    lengths: segment_template
                        .SegmentTimeline
                        .clone()
                        .expect("dash segment timeline")
                        .segments
                        .into_iter()
                        .flat_map(|s| {
                            std::iter::repeat(s.d as u32)
                                .take(s.r.unwrap_or_default() as usize + 1)
                                .collect::<Vec<u32>>()
                        })
                        .collect(),
                },
            });

            #[cfg(feature = "__test_strict")]
            stream_data.push(VariantData {
                executor: executor.clone(),
                resolution: Resolution {
                    // unwrap_or_default is called here because a audio representation has no
                    // resolution
                    height: representation.height.unwrap_or_default(),
                    width: representation.width.unwrap_or_default(),
                },
                bandwidth: representation.bandwidth.unwrap(),
                fps,
                codecs: representation.codecs.unwrap(),
                url: VariantDataUrl::MpegDash {
                    id: representation.id.expect("dash representation id"),
                    base: representation
                        .BaseURL
                        .first()
                        .expect("dash base url")
                        .base
                        .clone(),
                    init: segment_template
                        .initialization
                        .clone()
                        .expect("dash initialization url"),
                    fragments: segment_template.media.clone().expect("dash media url"),
                    start: segment_template.startNumber.expect("dash start number") as u32,
                    lengths: segment_template
                        .SegmentTimeline
                        .clone()
                        .expect("dash segment timeline")
                        .segments
                        .into_iter()
                        .flat_map(|s| {
                            std::iter::repeat(s.d as u32)
                                .take(s.r.unwrap_or_default() as usize + 1)
                                .collect::<Vec<u32>>()
                        })
                        .collect(),
                },
            })
        }

        Ok(stream_data)
    }

    /// Return all segments in order the variant stream is made of.
    pub async fn segments(&self) -> Result<Vec<VariantSegment>> {
        match &self.url {
            #[cfg(feature = "hls-stream")]
            VariantDataUrl::Hls { .. } => self.hls_segments().await,
            #[cfg(feature = "dash-stream")]
            VariantDataUrl::MpegDash { .. } => self.dash_segments().await,
        }
    }

    #[cfg(feature = "hls-stream")]
    async fn hls_segments(&self) -> Result<Vec<VariantSegment>> {
        use aes::cipher::KeyIvInit;

        #[allow(irrefutable_let_patterns)]
        let VariantDataUrl::Hls { url } = &self.url
        else {
            return Err(Error::Internal {
                message: "variant url should be hls".to_string(),
            });
        };

        let raw_media_playlist = self.executor.get(url).request_raw().await?;
        let media_playlist = m3u8_rs::parse_media_playlist_res(raw_media_playlist.as_slice())
            .map_err(|e| Error::Decode {
                message: e.to_string(),
                content: raw_media_playlist.clone(),
                url: url.clone(),
            })?;

        let mut segments: Vec<VariantSegment> = vec![];
        let mut key: Option<Aes128CbcDec> = None;

        for segment in media_playlist.segments {
            if let Some(k) = segment.key {
                if let Some(url) = k.uri {
                    let raw_key = self.executor.get(url).request_raw().await?;

                    let temp_iv = k.iv.unwrap_or_default();
                    let iv = if !temp_iv.is_empty() {
                        temp_iv.as_bytes()
                    } else {
                        raw_key.as_ref()
                    };

                    key = Some(Aes128CbcDec::new(raw_key.as_slice().into(), iv.into()));
                }
            }

            segments.push(VariantSegment {
                executor: self.executor.clone(),
                key: key.clone(),
                url: segment.uri,
                length: Duration::from_secs_f32(segment.duration),
            })
        }

        Ok(segments)
    }

    /// Get the m3u8 master url if you want to use it in an external download service (like ffmpeg)
    /// to handle the download process. Only works if this [`VariantData`] was returned by
    /// [`Stream::hls_streaming_data`].
    /// Implementing the download in native Rust has generally no drawbacks (if done with
    /// multithreading) and even can be faster than 3rd party tools (like ffmpeg; multithreaded
    /// native Rust is ~30 secs faster).
    #[cfg(feature = "hls-stream")]
    #[cfg_attr(docsrs, doc(cfg(feature = "hls-stream")))]
    pub fn hls_master_url(&self) -> Option<String> {
        match &self.url {
            VariantDataUrl::Hls { url } => Some(url.clone()),
            #[cfg(feature = "dash-stream")]
            _ => None,
        }
    }

    #[cfg(feature = "dash-stream")]
    async fn dash_segments(&self) -> Result<Vec<VariantSegment>> {
        #[allow(irrefutable_let_patterns)]
        let VariantDataUrl::MpegDash {
            id,
            base,
            init,
            fragments,
            start,
            lengths,
        } = self.url.clone()
        else {
            return Err(Error::Internal {
                message: "variant url should be dash".to_string(),
            });
        };

        let mut segments = vec![VariantSegment {
            executor: self.executor.clone(),
            key: None,
            url: base.clone() + &init.replace("$RepresentationID$", &id),
            length: Duration::from_secs(0),
        }];

        for (i, number) in (start..lengths.len() as u32 + start + 1).enumerate() {
            segments.push(VariantSegment {
                executor: self.executor.clone(),
                key: None,
                url: base.clone()
                    + &fragments
                        .replace("$Number$", &number.to_string())
                        .replace("$RepresentationID$", &id),
                length: Duration::from_millis(lengths.get(i).map_or(0, |l| *l) as u64),
            })
        }

        Ok(segments)
    }
}

/// A single segment, representing a part of a video stream.
/// Because Crunchyroll uses segment / chunk based video streaming (usually
/// [HLS](https://en.wikipedia.org/wiki/HTTP_Live_Streaming) or
/// [MPEG-DASH](https://en.wikipedia.org/wiki/Dynamic_Adaptive_Streaming_over_HTTP)) the actual
/// video stream consists of multiple [`VariantSegment`]s.
#[allow(dead_code)]
#[derive(Clone, Debug, Request)]
pub struct VariantSegment {
    executor: Arc<Executor>,

    /// Decryption key to decrypt the segment data (if encrypted).
    pub key: Option<Aes128CbcDec>,
    /// Url to the actual data.
    pub url: String,
    /// Video length of this segment.
    pub length: Duration,
}

impl VariantSegment {
    /// Decrypt a raw segment and return the decrypted raw bytes back. Useful if you want to
    /// implement the full segment download yourself and [`VariantSegment::write_to`] has too many
    /// limitation for your use case (e.g. a if you want to get the download speed of each segment).
    pub fn decrypt(segment_bytes: &mut [u8], key: Option<Aes128CbcDec>) -> Result<&[u8]> {
        use aes::cipher::BlockDecryptMut;
        if let Some(key) = key {
            // yes, the input bytes are copied into a new vec just for a better error output.
            // probably not worth it but better safe than sorry
            let error_segment_content = segment_bytes.to_vec();
            let decrypted = key
                .decrypt_padded_mut::<aes::cipher::block_padding::Pkcs7>(segment_bytes)
                .map_err(|e| Error::Decode {
                    message: e.to_string(),
                    content: error_segment_content,
                    url: "n/a".to_string(),
                })?;
            Ok(decrypted)
        } else {
            Ok(segment_bytes)
        }
    }

    /// Write this segment to a writer.
    pub async fn write_to(&self, w: &mut impl Write) -> Result<()> {
        let mut segment = self.executor.get(&self.url).request_raw().await?;

        w.write(VariantSegment::decrypt(
            segment.borrow_mut(),
            self.key.clone(),
        )?)
        .map_err(|e| Error::Input {
            message: e.to_string(),
        })?;

        Ok(())
    }
}
