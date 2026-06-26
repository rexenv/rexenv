import { Mail as MailIcon } from "lucide-react";
import { TopBar } from "@/components/shell/TopBar";
import { Placeholder } from "@/components/common/Placeholder";

export function Mail() {
  return (
    <>
      <TopBar title="Mail" subtitle="7 captured" />
      <Placeholder
        icon={<MailIcon className="h-[22px] w-[22px]" strokeWidth={1.6} />}
        label="Mail · content region"
        hint="Captured outgoing email (Mailpit) renders here"
      />
    </>
  );
}
