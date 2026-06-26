import { Settings as SettingsIcon } from "lucide-react";
import { TopBar } from "@/components/shell/TopBar";
import { Placeholder } from "@/components/common/Placeholder";

export function Settings() {
  return (
    <>
      <TopBar title="Settings" showSearch={false} />
      <Placeholder
        icon={<SettingsIcon className="h-[22px] w-[22px]" strokeWidth={1.6} />}
        label="Settings · content region"
        hint="General, DNS & SSL, Services, Updates, About render here"
      />
    </>
  );
}
