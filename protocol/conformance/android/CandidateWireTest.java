import com.google.protobuf.InvalidProtocolBufferException;
import dev.vista.candidate.v1.Vista;
import java.nio.file.Files;
import java.nio.file.Path;

/** JavaLite wire proof, deliberately not an Android runtime claim. */
public final class CandidateWireTest {
    static byte[] decodeHex(String s) {
        byte[] b = new byte[s.length()/2];
        for (int i=0; i<b.length; i++) b[i]=(byte)Integer.parseInt(s.substring(i*2,i*2+2),16);
        return b;
    }
    static void check(boolean condition, String note) { if (!condition) throw new AssertionError(note); }
    public static void main(String[] args) throws Exception {
        int count=0;
        for (String line: Files.readAllLines(Path.of(args[0]))) {
            String[] p=line.split("\\t"); byte[] b=decodeHex(p[2]);
            switch (p[0]) {
                case "version" -> check(Vista.ProtocolVersion.parseFrom(b).toByteArray().length==b.length, p[0]);
                case "optional-zero" -> { var v=Vista.VistaError.parseFrom(b); check(v.hasRetryAfterMs() && v.getRetryAfterMs()==0, p[0]); }
                case "grant-approved" -> { var v=Vista.PermissionGrant.parseFrom(b); check(v.getEpoch()==9007199254740993L, p[0]); }
                case "unknown-enum" -> check(Vista.PermissionGrant.parseFrom(b).getStateValue()==123,p[0]);
                case "enum-zero" -> check(Vista.PermissionGrant.parseFrom(b).getStateValue()==0,p[0]);
                case "grant-revoked" -> check(Vista.PermissionUpdate.parseFrom(b).getGrantsCount()==1,p[0]);
                case "catalog", "action" -> check(Vista.ControlEnvelope.parseFrom(b).hasVersion(),p[0]);
                case "picker", "verified-binding" -> check(Vista.CaptureSelection.parseFrom(b).hasStreamTarget(),p[0]);
                case "enroll" -> check(Vista.BootstrapEnvelope.parseFrom(b).hasPairingEnroll(),p[0]);
                case "certificate" -> check(Vista.PairingResult.parseFrom(b).getClientCertificateDer().size()>0,p[0]);
                case "lease" -> check(Vista.LeaseResult.parseFrom(b).hasActive(),p[0]);
                case "input" -> check(Vista.InputEnvelope.parseFrom(b).hasTarget(),p[0]);
                case "required-feature" -> check(Vista.ControlHello.parseFrom(b).getRequiredFeatures(0).getMustSupport(),p[0]);
                case "missing-domain" -> check(!Vista.LeaseRequest.parseFrom(b).hasTarget(),p[0]);
                case "unknown-field" -> check(Vista.ProtocolVersion.parseFrom(b).getMajor()==1,p[0]);
                case "additive-v1.1" -> check(Vista.ControlHello.parseFrom(b).getSelectedVersion().getMajor()==1,p[0]);
                default -> throw new AssertionError("Unknown fixture: "+p[0]);
            }
            count++;
        }
        try { Vista.ProtocolVersion.parseFrom(new byte[]{0x08}); throw new AssertionError("truncated accepted"); }
        catch (InvalidProtocolBufferException expected) { }
        System.out.println("JavaLite fixtures passed: "+count);
    }
}
