{
  Parity fixture (issue #31): `CreateProcessWait` of
  `xejviScriptAdapterMisc.pas` (`JvInterpreter_CreateProcessWait`).

  Upstream builds a `TStartUpInfo` with `STARTF_USESHOWWINDOW or
  STARTF_FORCEONFEEDBACK` and `wShowWindow := ShowWindow` (the third
  argument), passes the file as `lpApplicationName` and the parameter
  string as the command line, waits up to the timeout and reports
  `GetExitCodeProcess` whatever the wait returned -- `STILL_ACTIVE` (259)
  while the child runs, which is why a timeout does not end it. A
  `CreateProcess` failure raises
  `Exception.Create('CreateProcess failed, error code <GetLastError>')`.

  With an application name, `CreateProcess` resolves the name against the
  current directory only -- no search path -- so a bare `cmd.exe` fails
  (`error code 2`) where a full path runs; both are probed, so the port's
  `lpApplicationName` shape is compared, not only its exit code. The rest
  of the probe reads the exit codes of a finished child and of a timed-out
  one, and the show state the child sees: a small Python program (written
  by the probe into the run folder, run by both sides from their own) that
  prints `GetStartupInfo`'s `cb`, `dwFlags` and `wShowWindow` into a file
  the probe reads back and logs, so the
  `STARTF_USESHOWWINDOW`/`STARTF_FORCEONFEEDBACK` flags and the forwarded
  show state compare exactly.

  `C:\Windows\System32` and the Python of the machine are absolute paths
  because neither side's run inherits a usable `PATH`; the probe logs
  whether the interpreter is there, so a missing one is visible rather than
  silently different.
}
unit SysProcessProbe;

var
  gOut: string;
  gPython: string;

procedure Log(const aTag, aText: string);
begin
  AddMessage(aTag + ' | ' + aText);
end;

function Q(const aText: string): string;
begin
  Result := '"' + aText + '"';
end;

function Flag(aValue: Boolean): string;
begin
  if aValue then
    Result := 'T'
  else
    Result := 'F';
end;

procedure WriteChild;
var
  sl: TStringList;
begin
  sl := TStringList.Create;
  sl.Add('import ctypes, sys');
  sl.Add('class SI(ctypes.Structure):');
  sl.Add('    _fields_ = [("cb", ctypes.c_uint32), ("lpReserved", ctypes.c_void_p), ("lpDesktop", ctypes.c_void_p), ("lpTitle", ctypes.c_void_p),');
  sl.Add('                ("dwX", ctypes.c_uint32), ("dwY", ctypes.c_uint32), ("dwXSize", ctypes.c_uint32), ("dwYSize", ctypes.c_uint32),');
  sl.Add('                ("dwXCountChars", ctypes.c_uint32), ("dwYCountChars", ctypes.c_uint32), ("dwFillAttribute", ctypes.c_uint32),');
  sl.Add('                ("dwFlags", ctypes.c_uint32), ("wShowWindow", ctypes.c_uint16), ("cbReserved2", ctypes.c_uint16),');
  sl.Add('                ("lpReserved2", ctypes.c_void_p), ("hStdInput", ctypes.c_void_p), ("hStdOutput", ctypes.c_void_p), ("hStdError", ctypes.c_void_p)]');
  sl.Add('si = SI()');
  sl.Add('ctypes.windll.kernel32.GetStartupInfoW(ctypes.byref(si))');
  sl.Add('open(sys.argv[1], "w").write("cb=%d flags=%d show=%d" % (si.cb, si.dwFlags, si.wShowWindow))');
  sl.SaveToFile(gOut + 'proc-startup.py');
  sl.Free;
end;

function ReadReport(const aName: string): string;
var
  sl: TStringList;
begin
  sl := TStringList.Create;
  try
    sl.LoadFromFile(gOut + aName);
    Result := sl.Strings[0];
  except on E: Exception do
    Result := 'read-exc=' + E.ClassName + ' ' + E.Message;
  end;
  sl.Free;
end;

procedure Child(const aTag: string; aShow: Integer);
var
  rc: Integer;
begin
  rc := CreateProcessWait(gPython, Q(gPython) + ' ' + Q(gOut + 'proc-startup.py') + ' ' + Q(gOut + 'proc-' + aTag + '.txt'), aShow, 60000);
  Log('child', aTag + ' rc=' + IntToStr(rc) + ' report=' + ReadReport('proc-' + aTag + '.txt'));
end;

function Initialize: integer;
var
  rc: Integer;
begin
  Result := 0;
  gOut := ScriptsPath;
  gPython := 'C:\Users\Jamie\AppData\Local\Programs\Python\Python312\python.exe';
  Log('setup', 'python-exists=' + Flag(FileExists(gPython)));
  try
    rc := CreateProcessWait('cmd.exe', '/c exit 7', 0, 3000);
    Log('process', 'bare=' + IntToStr(rc));
  except on E: Exception do
    Log('process', 'bare-exc=' + E.ClassName + ' ' + E.Message);
  end;
  rc := CreateProcessWait('C:\Windows\System32\cmd.exe', '/c exit 7', 0, 30000);
  Log('process', 'exit7=' + IntToStr(rc));
  rc := CreateProcessWait('C:\Windows\System32\cmd.exe', '/c exit 0', 3, 30000);
  Log('process', 'exit0=' + IntToStr(rc));
  rc := CreateProcessWait('C:\Windows\System32\cmd.exe', '/c ping -n 4 127.0.0.1 >nul', 0, 300);
  Log('process', 'timeout=' + IntToStr(rc));
  try
    rc := CreateProcessWait('C:\Windows\System32\xedit-parity-no-such.exe', '', 0, 1000);
    Log('process', 'missing=' + IntToStr(rc));
  except on E: Exception do
    Log('process', 'missing-exc=' + E.ClassName + ' ' + E.Message);
  end;
  WriteChild;
  Child('hide', 0);
  Child('min', 2);
end;

end.
