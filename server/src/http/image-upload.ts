import { sshCommandArgv } from "../bridge/ssh-command";

const MAX_IMAGE_BYTES = 25 * 1024 * 1024;
const IMAGE_CONTENT_TYPE = /^image\/[a-z0-9.+-]+$/i;

function isPublicAddress(address: string): boolean {
  const normalized = address.toLowerCase();
  if (
    normalized === "::1" ||
    normalized === "::" ||
    normalized.startsWith("fe80:")
  ) {
    return false;
  }
  const parts = normalized.split(".").map(Number);
  if (parts.length !== 4 || parts.some(Number.isNaN)) return true;
  return !(
    parts[0] === 0 ||
    parts[0] === 10 ||
    parts[0] === 127 ||
    (parts[0] === 169 && parts[1] === 254) ||
    (parts[0] === 172 && parts[1] >= 16 && parts[1] <= 31) ||
    (parts[0] === 192 && parts[1] === 168)
  );
}

type ImageUploadDependencies = {
  sshHost: () => string | undefined;
  fetchImage?: (url: URL) => Promise<Response>;
  resolveHost?: (hostname: string) => Promise<string[]>;
  writeFile?: (path: string, data: Uint8Array) => Promise<unknown>;
};

export function createImageUploadHandler(args: ImageUploadDependencies) {
  return async function handleImageUpload(req: Request): Promise<Response> {
    try {
      const imageUrl = req.headers.get("x-image-url");
      let buf: Uint8Array;
      let ext: string;
      if (imageUrl) {
        let url: URL;
        try {
          url = new URL(imageUrl);
        } catch {
          return Response.json({ error: "invalid image URL" }, { status: 400 });
        }
        if (!["http:", "https:"].includes(url.protocol)) {
          return Response.json(
            { error: "image URL must use HTTP or HTTPS" },
            { status: 400 },
          );
        }
        const addresses = args.resolveHost
          ? await args.resolveHost(url.hostname)
          : (await Bun.dns.lookup(url.hostname)).map(({ address }) => address);
        if (
          addresses.length === 0 ||
          addresses.some((address) => !isPublicAddress(address))
        ) {
          return Response.json(
            { error: "image URL must resolve to a public address" },
            { status: 400 },
          );
        }
        const response = args.fetchImage
          ? await args.fetchImage(url)
          : await fetch(url, { redirect: "error" });
        if (!response.ok) {
          return Response.json(
            { error: `image URL returned ${response.status}` },
            { status: 502 },
          );
        }
        const contentType = response.headers
          .get("content-type")
          ?.split(";", 1)[0];
        if (!contentType || !IMAGE_CONTENT_TYPE.test(contentType)) {
          return Response.json(
            { error: "image URL did not return an image" },
            { status: 415 },
          );
        }
        const length = Number(response.headers.get("content-length"));
        if (Number.isFinite(length) && length > MAX_IMAGE_BYTES) {
          return Response.json(
            { error: "image too large (>25MB)" },
            { status: 413 },
          );
        }
        buf = new Uint8Array(await response.arrayBuffer());
        ext = contentType.split("/")[1]?.replace(/[^a-z0-9]/gi, "") || "png";
      } else {
        buf = new Uint8Array(await req.arrayBuffer());
        ext =
          (req.headers.get("x-image-ext") || "png")
            .toLowerCase()
            .replace(/[^a-z0-9]/g, "") || "png";
      }
      if (buf.length === 0) {
        return Response.json({ error: "empty body" }, { status: 400 });
      }
      if (buf.length > MAX_IMAGE_BYTES) {
        return Response.json(
          { error: "image too large (>25MB)" },
          { status: 413 },
        );
      }
      const name = `herdr-img-${Date.now()}-${Math.random()
        .toString(36)
        .slice(2, 8)}.${ext}`;
      const sshHost = args.sshHost();

      if (sshHost) {
        const remotePath = `/tmp/${name}`;
        const proc = Bun.spawn(sshCommandArgv(sshHost, `cat > ${remotePath}`), {
          stdin: buf,
          stdout: "pipe",
          stderr: "pipe",
        });
        const code = await proc.exited;
        if (code !== 0) {
          const err = await new Response(proc.stderr).text();
          return Response.json(
            { error: `ssh upload failed: ${err.trim() || `exit ${code}`}` },
            { status: 502 },
          );
        }
        return Response.json({ path: remotePath, remote: true });
      }

      const localPath = `/tmp/${name}`;
      await (args.writeFile ?? Bun.write)(localPath, buf);
      return Response.json({ path: localPath, remote: false });
    } catch (e) {
      return Response.json({ error: (e as Error).message }, { status: 500 });
    }
  };
}
