{ parity probe: r-01 }
unit P_r_01;

function Initialize: integer;
var
  files: Variant;
begin
  files := TDirectory.GetFiles(ProgramPath, '*.exe', 0);
  AddMessage('type ' + IntToStr(VarType(files)));
end;
end.
