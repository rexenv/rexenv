import { Database } from "lucide-react";
import { TopBar } from "@/components/shell/TopBar";
import { Placeholder } from "@/components/common/Placeholder";

export function Databases() {
  return (
    <>
      <TopBar title="Databases" subtitle="3 databases" />
      <Placeholder
        icon={<Database className="h-[22px] w-[22px]" strokeWidth={1.6} />}
        label="Databases · content region"
        hint="Your databases and the embedded browser render here"
      />
    </>
  );
}
