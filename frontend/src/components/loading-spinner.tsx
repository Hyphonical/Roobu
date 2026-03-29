interface LoadingSpinnerProps {
  className?: string;
}

export function LoadingSpinner({ className = "" }: LoadingSpinnerProps) {
  return (
    <div className={`flex items-center justify-center p-4 ${className}`}>
      <div className="h-6 w-6 animate-spin rounded-full border-2 border-[var(--color-muted-foreground)] border-t-transparent" />
    </div>
  );
}