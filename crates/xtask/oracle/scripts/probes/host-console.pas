{
  Parity probe (issue #25): ExecuteCaptureConsoleOutput runs a command line
  with its output captured into the message log (each read of the pipe,
  trimmed, one wbProgress call) and returns the exit code.
}
unit HostConsole;

function Initialize: Integer;
var
  code: Variant;
begin
  Result := 0;
  code := ExecuteCaptureConsoleOutput('cmd.exe /c echo one');
  AddMessage('exit code ' + VarToStr(code));
  code := ExecuteCaptureConsoleOutput('cmd.exe /c exit 7');
  AddMessage('exit code ' + VarToStr(code));
  code := ExecuteCaptureConsoleOutput('cmd.exe /c "echo   padded   & echo second 1>&2"');
  AddMessage('exit code ' + VarToStr(code));
  code := ExecuteCaptureConsoleOutput('cmd.exe /c echo.');
  AddMessage('exit code ' + VarToStr(code));
  try
    code := ExecuteCaptureConsoleOutput('Z:\no\such\program25.exe');
    AddMessage('exit code ' + VarToStr(code));
  except
    on E: Exception do
      AddMessage('missing program: ' + E.Message);
  end;
end;

end.
