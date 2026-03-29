import { Wifi, WifiOff, Loader2 } from "lucide-react";
import { WebSocketStatus } from "@/lib/types";

interface ConnectionStatusProps {
  status: WebSocketStatus;
}

export function ConnectionStatus({ status }: ConnectionStatusProps) {
  const statusConfig = {
    connecting: {
      icon: <Loader2 className="h-4 w-4 animate-spin" />,
      text: "Connecting...",
      className: "text-[var(--color-muted-foreground)]",
    },
    connected: {
      icon: <Wifi className="h-4 w-4" />,
      text: "Connected",
      className: "text-foreground",
    },
    disconnected: {
      icon: <WifiOff className="h-4 w-4" />,
      text: "Disconnected",
      className: "text-[var(--color-muted-foreground)]",
    },
    error: {
      icon: <WifiOff className="h-4 w-4" />,
      text: "Connection Error",
      className: "text-red-600",
    },
  };

  const config = statusConfig[status];

  return (
    <div className={`flex items-center gap-1.5 text-sm ${config.className}`}>
      {config.icon}
      <span>{config.text}</span>
    </div>
  );
}