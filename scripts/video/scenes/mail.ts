/** Mail catching: Mailpit already holds a few messages from the demo sites;
 *  `window.__scene.deliver()` drops a WordPress password-reset mail into it,
 *  the way wp_mail() → the pool's sendmail shim → Mailpit :11025 would.
 *  Mailpit marks a message read when its detail is fetched, and counts
 *  total/unread across the whole mailbox (`ipc/index.ts`). */
import type { SceneCtx } from "../demo-backend";
import type { MailDetail, MailSummary } from "@/types";

const ago = (min: number) => new Date(Date.now() - min * 60_000).toISOString();

type Msg = MailSummary & { text: string; html: string; to: MailSummary["to"] };

const RESET_TEXT = [
  "Someone has requested a password reset for the following account:",
  "",
  "Site Name: Shop Staging",
  "",
  "Username: admin",
  "",
  "If this was a mistake, ignore this email and nothing will happen.",
  "",
  "To reset your password, visit the following address:",
  "",
  "https://shop-staging.rex/wp-login.php?login=admin&key=Qm8xT3vN2pLr7kYs&action=rp",
  "",
  "This password reset request originated from the IP address 127.0.0.1.",
].join("\n");

const ORDER_HTML = `
<div style="font-family:Helvetica,Arial,sans-serif;background:#f7f7f7;padding:28px">
 <div style="max-width:560px;margin:0 auto;background:#fff;border-radius:6px;overflow:hidden">
  <div style="background:#7f54b3;color:#fff;padding:26px 30px;font-size:24px">New order: #1042</div>
  <div style="padding:26px 30px;color:#3c3c3c;font-size:14px;line-height:1.6">
   <p>You’ve received the following order from Rina Akter:</p>
   <h3 style="color:#7f54b3;margin:18px 0 8px">[Order #1042] (30 September 2026)</h3>
   <table style="width:100%;border-collapse:collapse;font-size:14px" cellpadding="8">
    <tr style="border-bottom:1px solid #eee"><th align="left">Product</th><th>Qty</th><th align="right">Price</th></tr>
    <tr style="border-bottom:1px solid #eee"><td>Linen tote bag</td><td align="center">2</td><td align="right">$48.00</td></tr>
    <tr style="border-bottom:1px solid #eee"><td>Ceramic mug</td><td align="center">1</td><td align="right">$18.00</td></tr>
    <tr><td colspan="2"><b>Total:</b></td><td align="right"><b>$66.00</b></td></tr>
   </table>
   <p style="margin-top:18px"><a href="https://shop-staging.rex/wp-admin/post.php?post=1042&action=edit" style="color:#7f54b3">View the order in your store</a></p>
  </div>
 </div>
</div>`;

export default function mail(ctx: SceneCtx) {
  const read = new Set<string>();
  const msgs: Msg[] = [
    {
      id: "m-order",
      from: { name: "Shop Staging", address: "wordpress@shop-staging.rex" },
      to: [{ name: "", address: "owner@example.com" }],
      subject: "[Shop Staging]: New order #1042",
      created: ago(18),
      read: false,
      snippet: "New order: #1042 — You’ve received the following order from Rina Akter",
      text: "New order: #1042\n\nYou’ve received the following order from Rina Akter.\n\nLinen tote bag × 2 — $48.00\nCeramic mug × 1 — $18.00\nTotal: $66.00",
      html: ORDER_HTML,
    },
    {
      id: "m-contact",
      from: { name: "Agency Blog", address: "wordpress@agency-blog.rex" },
      to: [{ name: "", address: "hello@agency-blog.rex" }],
      subject: 'Agency Blog "Project enquiry"',
      created: ago(42),
      read: true,
      snippet: "From: Tanvir Hasan — We'd like a quote for a new website…",
      text: "From: Tanvir Hasan <tanvir@example.com>\nSubject: Project enquiry\n\nMessage Body:\nWe'd like a quote for a new website before the holidays.",
      html: "",
    },
    {
      id: "m-booking",
      from: { name: "Booking API", address: "no-reply@booking-api.rex" },
      to: [{ name: "Mitu", address: "mitu@example.com" }],
      subject: "Your booking is confirmed",
      created: ago(64),
      read: true,
      snippet: "Hi Mitu, your booking for 12 October is confirmed…",
      text: "Hi Mitu,\n\nYour booking for 12 October at 3:00 PM is confirmed.\n\nThanks,\nBooking API",
      html: "<p>Hi Mitu,</p><p>Your booking for <b>12 October at 3:00 PM</b> is confirmed.</p><p>Thanks,<br>Booking API</p>",
    },
  ];
  for (const m of msgs) if (m.read) read.add(m.id);

  (window as unknown as { __scene: object }).__scene = {
    deliver() {
      msgs.unshift({
        id: "m-reset",
        from: { name: "Shop Staging", address: "wordpress@shop-staging.rex" },
        to: [{ name: "", address: "owner@example.com" }],
        subject: "[Shop Staging] Password Reset",
        created: new Date().toISOString(),
        read: false,
        snippet: "Someone has requested a password reset for the following account:",
        text: RESET_TEXT,
        html: "",
      });
    },
  };

  const summary = (m: Msg): MailSummary => ({
    id: m.id,
    from: m.from,
    to: m.to,
    subject: m.subject,
    created: m.created,
    read: read.has(m.id),
    snippet: m.snippet,
  });
  const detail = (m: Msg): MailDetail => ({
    id: m.id,
    from: m.from,
    to: m.to,
    cc: [],
    subject: m.subject,
    date: m.created,
    text: m.text,
    html: m.html,
    headers: [
      { name: "From", value: m.from.name ? `${m.from.name} <${m.from.address}>` : m.from.address },
      { name: "To", value: m.to.map((t) => t.address).join(", ") },
      { name: "Subject", value: m.subject },
      { name: "Date", value: new Date(m.created).toUTCString() },
      { name: "Content-Type", value: m.html ? "text/html; charset=UTF-8" : "text/plain; charset=UTF-8" },
      { name: "X-Mailer", value: "PHPMailer 6.9.3 (https://github.com/PHPMailer/PHPMailer)" },
    ],
  });

  return {
    mailpit_messages: (args: Record<string, unknown> | undefined) => {
      const q = String(args?.query ?? "").toLowerCase();
      const hit = msgs.filter((m) => !q || m.subject.toLowerCase().includes(q) || m.from.address.includes(q));
      const shown = args?.unreadOnly ? hit.filter((m) => !read.has(m.id)) : hit;
      return { total: msgs.length, unread: msgs.filter((m) => !read.has(m.id)).length, messages: shown.map(summary) };
    },
    mailpit_message: (args: Record<string, unknown> | undefined) => {
      const m = msgs.find((x) => x.id === args?.id) ?? msgs[0];
      read.add(m.id);
      return detail(m);
    },
    mailpit_message_raw: (args: Record<string, unknown> | undefined) => {
      const m = msgs.find((x) => x.id === args?.id) ?? msgs[0];
      return detail(m).headers.map((h) => `${h.name}: ${h.value}`).join("\r\n") + "\r\n\r\n" + m.text;
    },
    mailpit_mark_all_read: () => {
      msgs.forEach((m) => read.add(m.id));
      return null;
    },
    mailpit_delete: () => null,
    mailpit_clear: () => null,
    open_external: () => null,
  };
}
