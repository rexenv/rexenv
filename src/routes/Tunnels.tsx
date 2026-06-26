import { Share2 } from "lucide-react";
import { TopBar } from "@/components/shell/TopBar";
import { Placeholder } from "@/components/common/Placeholder";

export function Tunnels() {
  return (
    <>
      <TopBar title="Tunnels" subtitle="1 active" />
      <Placeholder
        icon={<Share2 className="h-[22px] w-[22px]" strokeWidth={1.6} />}
        label="Tunnels · content region"
        hint="Public Cloudflare tunnels render here"
      />
    </>
  );
}
