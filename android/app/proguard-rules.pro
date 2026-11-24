-repackageclasses
-allowaccessmodification

-keep class dev.yzewe.rynet.core.Native { *; }
-keepclassmembers class * extends android.net.VpnService {
    public boolean protect(int);
    public java.lang.String ownerPackage(boolean, java.lang.String, int, java.lang.String, int);
    public void onCoreExit(int, java.lang.String);
    public void onServerFailing();
    public void onKeepAlive(boolean);
}
-keep interface dev.yzewe.rynet.core.PingSink { *; }
-keepclassmembers class * implements dev.yzewe.rynet.core.PingSink { public void result(java.lang.String, int, java.lang.String); }
-keep interface dev.yzewe.rynet.core.SpeedSink { *; }
-keepclassmembers class * implements dev.yzewe.rynet.core.SpeedSink { public void progress(int, double, double); }
-keepclassmembers class * implements dev.yzewe.rynet.core.SpeedSink { public void server(java.lang.String); }
