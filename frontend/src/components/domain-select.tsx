"use client";

import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Label } from "@/components/ui/label";

interface DomainSelectProps {
  domains: string[];
  value: string;
  onChange: (value: string) => void;
  disabled?: boolean;
}

export function DomainSelect({ domains, value, onChange, disabled }: DomainSelectProps) {
  return (
    <div className="flex flex-col gap-1.5">
      <Label htmlFor="domain-select" className="text-xs text-[var(--color-muted-foreground)]">
        Domain
      </Label>
      <Select value={value} onValueChange={onChange} disabled={disabled}>
        <SelectTrigger id="domain-select" className="w-[180px]">
          <SelectValue placeholder="Select domain" />
        </SelectTrigger>
        <SelectContent>
          {domains.map((domain) => (
            <SelectItem key={domain} value={domain}>
              {domain === "all" ? "All Domains" : domain}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </div>
  );
}