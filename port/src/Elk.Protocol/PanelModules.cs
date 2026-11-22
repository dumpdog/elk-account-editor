namespace Elk.Protocol;

/// <summary>One device on the M1 data bus (or the control itself) with the versions it reports.</summary>
public sealed record BusModule(string Kind, int Address, string Model, string Hardware, string Boot, string Firmware, bool Responding);

/// <summary>
/// Bus module enrollment and the installed-device list, from ElkRP's "Enroll/Update Control and Devices" window (SystemVers.cs).
/// Enroll is <c>7F 00 00 11</c>: the panel scans the data bus, enrolls what it finds (the same as keypad installer programming menu 01) and drops the connection;
/// ElkRP waits about 10 seconds and reconnects. The enrolled devices are the bits of the reply to <c>7F 00 00 04</c>; each device's versions come from
/// <c>7C 00 type address</c> (type 1 keypad, 2 input expander, 3 output expander, 5 serial port expander) and <c>75 06</c> (M1XEP).
/// </summary>
public static class PanelModules
{
    /// <summary>Kind, device type used in the version query, first item in ElkRP's ValidItems table, number of devices.</summary>
    private static readonly (string Kind, byte Type, int Group, int Count)[] Kinds =
    {
        ("Keypad", 1, 0, 16), ("Input expander", 2, 1, 13), ("Output expander", 3, 2, 13), ("Serial port expander", 5, 4, 7),
    };

    /// <summary>Which devices the panel has enrolled, by kind and address (1-based). Group g of the reply holds 32 devices, highest byte first.</summary>
    public static List<(string Kind, byte Type, int Address)> DecodeEnrolled(byte[] reply)
    {
        // ElkRP picks the first byte of the groups from the reply length (SystemVers.GetActiveExps)
        int num = reply.Length <= 76 ? 47 : reply.Length <= 88 ? 59 : reply.Length <= 171 ? 147 : 151;
        var list = new List<(string, byte, int)>();
        foreach (var (kind, type, group, count) in Kinds)
            for (int n = 1; n <= count; n++)
            {
                int b = n - 1, idx = num + 4 + 4 * group - b / 8;
                if (idx >= 0 && idx < reply.Length && (reply[idx] & (1 << (b % 8))) != 0) list.Add((kind, type, n));
            }
        return list;
    }

    private static string KeypadModel(int hw) => hw switch { <= 255 => "M1KP", <= 511 => "M1KP2/KP3", <= 767 => "M1KPAS", <= 1023 => "M1KAM", <= 1279 => "M1KPNAV", <= 1535 => "P212S", _ => "Unknown" };
    private static string InputModel(int hw) => hw switch { <= 255 => "M1XIN", <= 511 => "M1XRF2G", <= 767 => "M1XRF1N", <= 1023 => "M1XRF2N", <= 1279 => "M1XRF2H", <= 1535 => "M1XRFEG", <= 1791 => "M1XRFTW", <= 2047 => "M1XRFTWM", <= 2303 => "M1XRF319", <= 2559 => "M1XRF2Hv2", _ => "Unknown" };
    private static string SerialModel(int hw) => hw is >= 2560 and <= 2815 ? "M1XSLU" : hw is >= 2816 and <= 3071 ? "M1XSLZW" : "M1XSP";

    public static string ModelFor(string kind, int hardware) => kind switch
    {
        "Keypad" => KeypadModel(hardware), "Input expander" => InputModel(hardware), "Output expander" => hardware <= 255 ? "M1XOV(R)" : "Unknown",
        "Serial port expander" => SerialModel(hardware), "Ethernet expander" => "M1XEP", _ => "Unknown",
    };

    private static string Ver3(int v) => $"{v / 65536}.{(v / 256) & 0xFF}.{v & 0xFF}";

    /// <summary>Decodes a device version reply (7C or 75): hardware r[8..9], boot r[10..12], firmware r[13..15].</summary>
    public static BusModule DecodeVersions(string kind, int address, byte[]? r)
    {
        if (r == null || r.Length < 16) return new BusModule(kind, address, "Unknown", "", "", "Missing or not working", false);
        int hw = r[8] * 256 + r[9], boot = r[10] * 65536 + r[11] * 256 + r[12], fw = r[13] * 65536 + r[14] * 256 + r[15];
        string firm = fw == 16777215 ? "Missing or not working" : fw == 0 ? "Erased" : Ver3(fw);
        return new BusModule(kind, address, ModelFor(kind, hw), $"{(hw / 256) & 0xFF}.{hw & 0xFF}", Ver3(boot), firm, fw != 16777215);
    }

    private static async Task<byte[]?> Ask(ElkSession s, byte[] request, byte expect, CancellationToken ct)
    {
        await s.SendAsync(request, ct);
        try { return await s.ReceiveAsync(expect, ct); } catch (TimeoutException) { return null; }
    }

    /// <summary>Reads the control, the M1XEP and every enrolled device with its versions. Read only.</summary>
    public static async Task<List<BusModule>> ScanAsync(ElkSession s, CancellationToken ct = default)
    {
        var list = new List<BusModule>();
        if (s.Panel is { } p) list.Add(new BusModule("Control", 0, "M1G", p.Hardware, p.Boot, p.Firmware, true));
        await s.SendAsync(new byte[] { 0x7F, 0, 0, 4 }, ct);
        var enrolled = await s.ReceiveAsync(0x7F, ct);
        foreach (var (kind, type, addr) in DecodeEnrolled(enrolled))
        {
            var r = await Ask(s, new byte[] { 0x7C, 0, type, (byte)addr }, 0x7C, ct);
            if (r != null && (r[2] != type || r[3] != addr)) r = null;
            list.Add(DecodeVersions(kind, addr, r));
        }
        var xep = await Ask(s, new byte[] { 0x75, 6 }, 0x75, ct);
        if (xep != null && xep.Length >= 16) list.Add(DecodeVersions("Ethernet expander", 0, xep));
        return list;
    }

    /// <summary>Starts bus module enrollment. The panel drops the connection and rescans the bus for several seconds; log in again afterwards (about 10 seconds later).
    /// Devices that are enrolled but missing at that moment are not kept, so only do this with every module powered and connected.</summary>
    public static async Task<string> EnrollAsync(ElkSession s, CancellationToken ct = default)
    {
        await s.SendAsync(new byte[] { 0x7F, 0, 0, 17 }, ct);
        try { var f = await s.ReceiveAnyFrameAsync(ct); return f.Kind == FrameKind.Ack ? "the panel acknowledged (ACK)" : f.Kind == FrameKind.Nak ? "the panel refused (NAK)" : "the panel answered: " + Convert.ToHexString(f.Payload); }
        catch (TimeoutException) { return "the panel sent no answer"; }
        catch (IOException) { return "the panel closed the connection"; }
    }
}
