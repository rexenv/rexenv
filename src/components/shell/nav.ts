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
  /** small mono count shown on the right — computed live in the Sidebar (`liveBadges`) */
  badge?: string;
  /** green pulsing dot before the badge — computed live in the Sidebar (tunnels active) */
  activeDot?: boolean;
  /** footer-pinned items (Settings) render below the spacer */
  footer?: boolean;
}

export const NAV_ITEMS: NavItem[] = [
  { to: "/sites", label: "Sites", icon: Globe, group: "Environment" },
  { to: "/services", label: "Services", icon: Server, group: "Environment" },
  { to: "/databases", label: "Databases", icon: Database, group: "Environment" },
  { to: "/mail", label: "Mail", icon: Mail, group: "Network" },
  { to: "/tunnels", label: "Tunnels", icon: Share2, group: "Network" },
  { to: "/settings", label: "Settings", icon: SlidersHorizontal, group: "Environment", footer: true },
];
