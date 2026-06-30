import {
  Database,
  Globe,
  Mail,
  Server,
  Share2,
  SlidersHorizontal,
  type LucideIcon,
} from "lucide-react";

export interface NavItem {
  to: string;
  label: string;
  icon: LucideIcon;
  group: "Environment" | "Network";
  /** small mono count shown on the right (mock for now) */
  badge?: string;
  /** green pulsing dot before the badge when the feature is live (mock for now) */
  activeDot?: boolean;
  /** footer-pinned items (Settings) render below the spacer */
  footer?: boolean;
}

export const NAV_ITEMS: NavItem[] = [
  { to: "/sites", label: "Sites", icon: Globe, group: "Environment", badge: "12" },
  { to: "/services", label: "Services", icon: Server, group: "Environment", badge: "4" },
  { to: "/databases", label: "Databases", icon: Database, group: "Environment", badge: "3" },
  { to: "/mail", label: "Mail", icon: Mail, group: "Network", badge: "7" },
  { to: "/tunnels", label: "Tunnels", icon: Share2, group: "Network", badge: "1", activeDot: true },
  { to: "/settings", label: "Settings", icon: SlidersHorizontal, group: "Environment", footer: true },
];
