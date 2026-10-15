{ parity probe: r-11 }
unit P_r_11;

function Initialize: integer;
var
  files: Variant;
begin
  files := TDirectory.GetFiles(ProgramPath, '*.exe', 0);
  AddMessage('n ' + IntToStr(Length(files)));
  AddMessage('high ' + IntToStr(High(files)));
  AddMessage('low ' + IntToStr(Low(files)));
end;
end.
