{ parity probe: r-04 }
unit P_r_04;

function Initialize: integer;
var
  d: TDirectory;
  files: Variant;
begin
  files := d.GetFiles(ProgramPath, '*.exe', 0);
  AddMessage('type ' + IntToStr(VarType(files)));
end;
end.
