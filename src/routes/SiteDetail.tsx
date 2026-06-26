import { useParams } from "react-router-dom";
import { LayoutGrid } from "lucide-react";
import { TopBar } from "@/components/shell/TopBar";
import { Placeholder } from "@/components/common/Placeholder";
import { mockSites } from "@/lib/mock";

export function SiteDetail() {
  const { id } = useParams();
  const site = mockSites.find((s) => s.id === id);
  return (
    <>
      <TopBar
        title={site?.name ?? "Site"}
        subtitle={site?.domain}
        showSearch={false}
      />
      <Placeholder
        icon={<LayoutGrid className="h-[22px] w-[22px]" strokeWidth={1.6} />}
        label="Site detail · content region"
        hint="Overview, WordPress, Database, Logs, Settings tabs render here"
      />
    </>
  );
}
