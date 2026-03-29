"use client";

import { useState, useCallback, KeyboardEvent } from "react";
import { Search, X } from "lucide-react";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";

interface SearchBoxProps {
  onSearch: (query: string) => void;
  disabled?: boolean;
}

export function SearchBox({ onSearch, disabled }: SearchBoxProps) {
  const [query, setQuery] = useState("");

  const handleSearch = useCallback(() => {
    if (query.trim()) {
      onSearch(query.trim());
    }
  }, [query, onSearch]);

  const handleKeyDown = useCallback(
    (e: KeyboardEvent<HTMLInputElement>) => {
      if (e.key === "Enter") {
        handleSearch();
      } else if (e.key === "Escape") {
        setQuery("");
      }
    },
    [handleSearch]
  );

  const handleClear = useCallback(() => {
    setQuery("");
  }, []);

  return (
    <div className="flex flex-col gap-1.5">
      <Label htmlFor="search-input" className="text-xs text-[var(--color-muted-foreground)]">
        Search
      </Label>
      <div className="flex gap-2">
        <div className="relative flex-1">
          <Search className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-[var(--color-muted-foreground)]" />
          <Input
            id="search-input"
            type="text"
            placeholder="Search posts..."
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={handleKeyDown}
            disabled={disabled}
            className="pl-9 pr-9"
          />
          {query && (
            <Button
              variant="ghost"
              size="icon"
              className="absolute right-1 top-1/2 h-7 w-7 -translate-y-1/2 text-[var(--color-muted-foreground)] hover:text-foreground"
              onClick={handleClear}
              disabled={disabled}
            >
              <X className="h-4 w-4" />
            </Button>
          )}
        </div>
        <Button onClick={handleSearch} disabled={disabled || !query.trim()}>
          Search
        </Button>
      </div>
    </div>
  );
}