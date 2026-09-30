"use client";

import { useState } from "react";
import videos from "../content/videos/videos.json";

function extractYouTubeId(url) {
  try {
    const u = new URL(url.trim());
    if (u.hostname === "youtu.be")
      return u.pathname.slice(1).split(/[?&#]/)[0] || null;
    if (u.hostname.endsWith("youtube.com"))
      return u.searchParams.get("v") || null;
  } catch {
    return null;
  }
  return null;
}

// YouTube entries derive their thumbnail from the video id; other platforms
// (e.g. Instagram reels) have no public thumbnail URL, so the entry carries a
// `thumbnail` path to a still committed under public/videos/.
function thumbnailFor(video) {
  return video.thumbnail ?? `https://img.youtube.com/vi/${video.id}/mqdefault.jpg`;
}

const videoGrid = videos.map((video) => (
  <div key={video.id} className="showcase-card">
    <div
      className={`showcase-card-video${video.vertical ? " showcase-card-video-vertical" : ""}`}
    >
      <a
        href={video.url}
        target="_blank"
        rel="noopener noreferrer"
        aria-label={`Watch ${video.title} by ${video.author}`}
      >
        {/* Vertical videos keep the grid's 16:9 frame: the still is shown
            whole in the middle over a blurred copy of itself. */}
        {video.vertical && (
          <img
            className="showcase-thumb-backdrop"
            src={thumbnailFor(video)}
            alt=""
            aria-hidden="true"
            loading="lazy"
          />
        )}
        <img
          className={video.vertical ? "showcase-thumb-vertical" : undefined}
          src={thumbnailFor(video)}
          alt={`${video.title} by ${video.author}`}
          loading="lazy"
        />
        <div className="showcase-play-icon">
          <svg
            xmlns="http://www.w3.org/2000/svg"
            width="24"
            height="24"
            viewBox="0 0 24 24"
            fill="currentColor"
          >
            <path d="M8 5v14l11-7z" />
          </svg>
        </div>
      </a>
    </div>
    <div className="showcase-card-info">
      <div className="showcase-card-title">{video.title}</div>
      <div className="showcase-card-author">by {video.author}</div>
    </div>
  </div>
));

export function Videos() {
  return (
    <div className="showcase-page showcase-page-wide">
      <div className="showcase-hero">
        <div className="showcase-hero-kicker">VIDEOS</div>
        <h1 className="showcase-hero-title">Made With Cyclemetry</h1>
        <p className="showcase-hero-desc">
          See what cyclists around the world are creating with Cyclemetry. Every
          video here is made with Cyclemetry — telemetry overlays synced to ride
          footage.
        </p>
      </div>

      <div className="showcase-grid">{videoGrid}</div>

      <div className="showcase-submit-panel">
        <div>
          <h2>Share your ride</h2>
          <p>
            Made a video with Cyclemetry? Send it in and we can feature it here.
          </p>
        </div>
        <div className="showcase-submit-actions">
          <a
            className="showcase-button showcase-button-primary"
            href="/videos/submit"
          >
            Submit your video
            <svg
              xmlns="http://www.w3.org/2000/svg"
              width="17"
              height="17"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="2"
              strokeLinecap="round"
              strokeLinejoin="round"
            >
              <path d="M5 12h14" />
              <path d="m12 5 7 7-7 7" />
            </svg>
          </a>
          <a
            className="showcase-button"
            href="https://github.com/walkersutton/cyclemetry/blob/main/website/content/videos/videos.json"
            target="_blank"
            rel="noopener noreferrer"
          >
            View JSON
          </a>
        </div>
      </div>

      <div className="showcase-callout">
        <div className="showcase-callout-icon">
          <svg
            xmlns="http://www.w3.org/2000/svg"
            width="18"
            height="18"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            strokeLinecap="round"
            strokeLinejoin="round"
          >
            <circle cx="12" cy="12" r="10" />
            <path d="M12 16v-3" />
            <path d="M12 8h.01" />
          </svg>
        </div>
        <div className="showcase-callout-text">
          <strong>Want to add your video?</strong> It only takes a minute —{" "}
          <a href="/videos/submit">see how to submit yours</a>.
        </div>
      </div>
    </div>
  );
}

export function SubmitVideo() {
  const [youtubeUrl, setYoutubeUrl] = useState("");
  const [title, setTitle] = useState("");
  const [author, setAuthor] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState(null);
  const [prUrl, setPrUrl] = useState(null);

  const videoId = extractYouTubeId(youtubeUrl);

  async function handleSubmit(e) {
    e.preventDefault();
    setError(null);
    setLoading(true);
    try {
      const res = await fetch("/api/videos/submit", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ youtubeUrl, title, author }),
      });
      const data = await res.json();
      if (!res.ok) {
        setError(data.error || "Something went wrong.");
      } else {
        setPrUrl(data.prUrl);
      }
    } catch {
      setError("Network error. Please try again.");
    } finally {
      setLoading(false);
    }
  }

  return (
    <div className="showcase-page showcase-page-narrow">
      <div className="showcase-hero">
        <div className="showcase-hero-kicker">SUBMIT</div>
        <h1 className="showcase-hero-title">Submit Your Video</h1>
        <p className="showcase-hero-desc">
          Add your ride video to the Cyclemetry community. Fill in the form and
          we&apos;ll open a PR automatically.
        </p>
      </div>

      {prUrl ? (
        <div className="submit-success">
          <div className="submit-success-icon">
            <svg
              xmlns="http://www.w3.org/2000/svg"
              width="20"
              height="20"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="2"
              strokeLinecap="round"
              strokeLinejoin="round"
            >
              <path d="M20 6 9 17l-5-5" />
            </svg>
          </div>
          <div className="submit-success-body">
            <strong>You&apos;re in the queue.</strong>&nbsp;Your video has been
            submitted as a pull request. Once it&apos;s reviewed and merged,
            it&apos;ll appear on the videos page.
            <div className="submit-success-actions">
              <a
                className="showcase-button showcase-button-primary"
                href={prUrl}
                target="_blank"
                rel="noopener noreferrer"
              >
                View pull request
                <svg
                  xmlns="http://www.w3.org/2000/svg"
                  width="15"
                  height="15"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="2"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                >
                  <path d="M5 12h14" />
                  <path d="m12 5 7 7-7 7" />
                </svg>
              </a>
              <a className="showcase-button" href="/videos">
                Back to videos
              </a>
            </div>
          </div>
        </div>
      ) : (
        <form className="submit-form" onSubmit={handleSubmit}>
          <div className="submit-field">
            <label className="submit-label" htmlFor="youtubeUrl">
              YouTube URL
            </label>
            <input
              id="youtubeUrl"
              className="submit-input"
              type="url"
              placeholder="https://www.youtube.com/watch?v=..."
              value={youtubeUrl}
              onChange={(e) => setYoutubeUrl(e.target.value)}
              required
              disabled={loading}
            />
            {videoId && (
              <div className="submit-preview">
                <img
                  src={`https://img.youtube.com/vi/${videoId}/mqdefault.jpg`}
                  alt="Video thumbnail"
                />
                <span className="submit-preview-id">
                  <svg
                    xmlns="http://www.w3.org/2000/svg"
                    width="14"
                    height="14"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="2"
                    strokeLinecap="round"
                    strokeLinejoin="round"
                  >
                    <path d="M20 6 9 17l-5-5" />
                  </svg>
                  ID: <code>{videoId}</code>
                </span>
              </div>
            )}
          </div>

          <div className="submit-field">
            <label className="submit-label" htmlFor="title">
              Video title
            </label>
            <input
              id="title"
              className="submit-input"
              type="text"
              placeholder="e.g. Morning climb up Mount Tamalpais"
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              required
              disabled={loading}
              maxLength={120}
            />
          </div>

          <div className="submit-field">
            <label className="submit-label" htmlFor="author">
              Your name
            </label>
            <input
              id="author"
              className="submit-input"
              type="text"
              placeholder="e.g. Alex Chen"
              value={author}
              onChange={(e) => setAuthor(e.target.value)}
              required
              disabled={loading}
              maxLength={80}
            />
          </div>

          {error && <p className="submit-error">{error}</p>}

          <button
            type="submit"
            className={`showcase-button showcase-button-primary submit-cta${loading ? " submit-cta-loading" : ""}`}
            disabled={loading}
          >
            {loading ? (
              <>
                <svg
                  className="submit-spinner"
                  xmlns="http://www.w3.org/2000/svg"
                  width="16"
                  height="16"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="2.5"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                >
                  <path d="M21 12a9 9 0 1 1-6.219-8.56" />
                </svg>
                Submitting…
              </>
            ) : (
              <>
                Submit video
                <svg
                  xmlns="http://www.w3.org/2000/svg"
                  width="17"
                  height="17"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="2"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                >
                  <path d="M5 12h14" />
                  <path d="m12 5 7 7-7 7" />
                </svg>
              </>
            )}
          </button>
        </form>
      )}

      <div className="submit-divider">
        <span>or submit manually</span>
      </div>

      <div className="showcase-content">
        <h2>Submit via GitHub</h2>
        <p>
          Prefer to open the PR yourself? Edit{" "}
          <a
            href="https://github.com/walkersutton/cyclemetry/blob/main/website/content/videos/videos.json"
            target="_blank"
            rel="noopener noreferrer"
          >
            videos.json
          </a>{" "}
          directly on GitHub and add your entry:
        </p>

        <pre className="showcase-code-block">
          <code>{`{
  "id": "YOUR_VIDEO_ID",
  "title": "Your Ride Title",
  "author": "Your Name",
  "url": "https://www.youtube.com/watch?v=YOUR_VIDEO_ID"
}`}</code>
        </pre>

        <div className="showcase-callout">
          <div className="showcase-callout-icon">
            <svg
              xmlns="http://www.w3.org/2000/svg"
              width="18"
              height="18"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="2"
              strokeLinecap="round"
              strokeLinejoin="round"
            >
              <circle cx="12" cy="12" r="10" />
              <path d="M12 16v-3" />
              <path d="M12 8h.01" />
            </svg>
          </div>
          <div className="showcase-callout-text">
            <strong>Questions?</strong> Open a{" "}
            <a
              href="https://github.com/walkersutton/cyclemetry/issues"
              target="_blank"
              rel="noopener noreferrer"
            >
              GitHub issue
            </a>{" "}
            and we&apos;ll help you get your video featured.
          </div>
        </div>
      </div>
    </div>
  );
}
