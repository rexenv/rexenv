import { Layers } from "lucide-react";
import { TopBar } from "@/components/shell/TopBar";
import { Placeholder } from "@/components/common/Placeholder";

export function Services() {
  return (
    <>
      <TopBar title="Services" subtitle="4 running" />
      <Placeholder
        icon={<Layers className="h-[22px] w-[22px]" strokeWidth={1.6} />}
        label="Services · content region"
        hint="PHP, databases, mail, and web servers render here"
      />
    </>
  );
}
