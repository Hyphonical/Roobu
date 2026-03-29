"use client";

import { SearchResult } from "@/lib/types";
import { ExternalLink } from "lucide-react";

interface ImageCardProps {
  post: SearchResult;
  tabIndex?: number;
}

const ratingLabels: Record<string, string> = {
  safe: "S",
  questionable: "Q",
  explicit: "E",
};

export function ImageCard({ post, tabIndex = 0 }: ImageCardProps) {
  const handleClick = () => {
    window.open(post.post_url, "_blank", "noopener,noreferrer");
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      handleClick();
    }
  };

  return (
    <div
      className="masonry-item group cursor-pointer"
      role="button"
      tabIndex={tabIndex}
      onClick={handleClick}
      onKeyDown={handleKeyDown}
    >
      <div className="relative overflow-hidden rounded-md bg-muted">
        {/* Aspect ratio container */}
        <div className="relative w-full bg-muted">
          <img
            src={post.thumbnail_url}
            alt={`Post ${post.post_id} from ${post.site}`}
            loading="lazy"
            className="h-auto w-full object-cover transition-transform duration-300 group-hover:scale-105"
          />
        </div>

        {/* Hover overlay */}
        <div className="absolute inset-0 bg-gradient-to-t from-black/70 via-transparent to-transparent opacity-0 transition-opacity duration-300 group-hover:opacity-100">
          <div className="absolute bottom-0 left-0 right-0 p-2">
            <div className="flex items-center justify-between">
              <span className="text-xs font-medium text-white">{post.site}</span>
              {post.rating && (
                <span className="rounded bg-white/20 px-1.5 py-0.5 text-[10px] font-semibold uppercase text-white">
                  {ratingLabels[post.rating] || post.rating}
                </span>
              )}
            </div>
          </div>
          <div className="absolute right-2 top-2">
            <ExternalLink className="h-4 w-4 text-white" />
          </div>
        </div>

        {/* Always visible site label at bottom */}
        <div className="absolute bottom-0 left-0 right-0 bg-gradient-to-t from-black/50 to-transparent p-2">
          <span className="text-[10px] text-white/80">{post.site}</span>
        </div>
      </div>
    </div>
  );
}