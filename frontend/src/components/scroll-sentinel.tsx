import { forwardRef } from "react";

interface ScrollSentinelProps {
  className?: string;
}

export const ScrollSentinel = forwardRef<HTMLDivElement, ScrollSentinelProps>(
  ({ className = "" }, ref) => {
    return (
      <div
        ref={ref}
        className={`h-px w-full ${className}`}
        aria-hidden="true"
      />
    );
  }
);

ScrollSentinel.displayName = "ScrollSentinel";