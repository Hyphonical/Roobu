"use client";

import { useCallback, useEffect, useState } from "react";
import { useWebSocket } from "@/hooks/use-websocket";
import { useInfiniteScroll } from "@/hooks/use-infinite-scroll";
import { WS_URL, DEFAULT_DOMAIN, DEFAULT_LIMIT, SCROLL_THRESHOLD } from "@/lib/constants";
import type { DomainsResponse, SearchResponse, WSMessage, SearchResult } from "@/lib/types";
import { DomainSelect } from "@/components/domain-select";
import { SearchBox } from "@/components/search-box";
import { MasonryGrid } from "@/components/masonry-grid";
import { ImageCard } from "@/components/image-card";
import { ScrollSentinel } from "@/components/scroll-sentinel";
import { LoadingSpinner } from "@/components/loading-spinner";
import { ConnectionStatus } from "@/components/connection-status";

export default function Home() {
  // WebSocket connection
  const { status, lastMessage, send } = useWebSocket(WS_URL);

  // Domain state
  const [domains, setDomains] = useState<string[]>(["all"]);

  // Selection state
  const [selectedDomain, setSelectedDomain] = useState(DEFAULT_DOMAIN);
  const [query, setQuery] = useState("");

  // Results state
  const [posts, setPosts] = useState<SearchResult[]>([]);
  const [offset, setOffset] = useState(0);
  const [hasMore, setHasMore] = useState(true);
  const [isLoading, setIsLoading] = useState(false);
  const [hasSearched, setHasSearched] = useState(false);

  // Handle WebSocket messages
  useEffect(() => {
    if (!lastMessage) return;

    const message = lastMessage as WSMessage;

    if (message.type === "domains") {
      const domainsResponse = message as DomainsResponse;
      const allDomains = ["all", ...domainsResponse.domains.filter((d) => d !== "all")];
      setDomains(allDomains);
    } else if (message.type === "results") {
      const results = message as SearchResponse;
      setPosts((prev) => (offset === 0 ? results.posts : [...prev, ...results.posts]));
      setHasMore(results.hasMore);
      setIsLoading(false);
      setHasSearched(true);
    } else if (message.type === "error") {
      setIsLoading(false);
      console.error("Search error:", message);
    }
  }, [lastMessage, offset]);

  // Fetch domains on connect
  useEffect(() => {
    if (status === "connected") {
      send({ type: "domains" });
    }
  }, [status, send]);

  // Execute search
  const executeSearch = useCallback(
    (newOffset: number, searchQuery?: string) => {
      if (status !== "connected") return;

      setIsLoading(true);
      setOffset(newOffset);

      send({
        type: "search",
        query: searchQuery ?? query,
        domain: selectedDomain,
        limit: DEFAULT_LIMIT,
        offset: newOffset,
      });
    },
    [status, query, selectedDomain, send]
  );

  // Handle search submission
  const handleSearch = useCallback(
    (searchQuery: string) => {
      setQuery(searchQuery);
      setOffset(0);
      setPosts([]);
      setHasMore(true);
      executeSearch(0, searchQuery);
    },
    [executeSearch]
  );

  // Handle domain change
  const handleDomainChange = useCallback(
    (domain: string) => {
      setSelectedDomain(domain);
      if (query) {
        setOffset(0);
        setPosts([]);
        setHasMore(true);
        executeSearch(0, query);
      }
    },
    [query, executeSearch]
  );

  // Load more for infinite scroll
  const loadMore = useCallback(() => {
    if (!isLoading && hasMore && hasSearched) {
      executeSearch(offset + DEFAULT_LIMIT);
    }
  }, [isLoading, hasMore, hasSearched, offset, executeSearch]);

  // Infinite scroll sentinel
  const sentinelRef = useInfiniteScroll(loadMore, {
    threshold: SCROLL_THRESHOLD,
    enabled: hasMore && !isLoading && hasSearched,
  });

  return (
    <div className="min-h-screen">
      {/* Header */}
      <header className="sticky top-0 z-10 border-b border-[var(--color-border)] bg-[var(--color-background)]/95 backdrop-blur">
        <div className="mx-auto flex max-w-7xl flex-col gap-4 p-4 sm:flex-row sm:items-center sm:justify-between">
          <div className="flex flex-col gap-4 sm:flex-row sm:items-center sm:gap-6">
            <h1 className="text-xl font-semibold">Gallery</h1>
            <DomainSelect
              domains={domains}
              value={selectedDomain}
              onChange={handleDomainChange}
              disabled={status !== "connected"}
            />
            <SearchBox onSearch={handleSearch} disabled={status !== "connected"} />
          </div>
          <ConnectionStatus status={status} />
        </div>
      </header>

      {/* Main content */}
      <main className="mx-auto max-w-7xl p-4">
        {/* Initial state */}
        {!hasSearched && !isLoading && (
          <div className="flex h-[50vh] items-center justify-center">
            <p className="text-[var(--color-muted-foreground)]">
              {status === "connected"
                ? "Enter a search query to find posts"
                : "Connecting to server..."}
            </p>
          </div>
        )}

        {/* No results */}
        {hasSearched && !isLoading && posts.length === 0 && (
          <div className="flex h-[50vh] items-center justify-center">
            <p className="text-[var(--color-muted-foreground)]">No results found</p>
          </div>
        )}

        {/* Results grid */}
        {posts.length > 0 && (
          <MasonryGrid>
            {posts.map((post, index) => (
              <ImageCard key={`${post.post_id}-${post.site}-${index}`} post={post} />
            ))}
          </MasonryGrid>
        )}

        {/* Loading indicator */}
        {isLoading && <LoadingSpinner className="py-8" />}

        {/* Infinite scroll sentinel */}
        {hasMore && posts.length > 0 && <ScrollSentinel ref={sentinelRef} />}

        {/* End of results */}
        {!hasMore && posts.length > 0 && (
          <div className="py-8 text-center">
            <p className="text-sm text-[var(--color-muted-foreground)]">End of results</p>
          </div>
        )}
      </main>
    </div>
  );
}