{ parity probe: x-230 }
unit P_x_230;

function Initialize: integer;
var
  i: Integer;
  d: Double;
  s: string;
  b: Boolean;
  sl: TStringList;
  v: Variant;
begin
  sl := TStringList.Create;
  sl.Add('a');
  v := FileByIndex(0); v := v = FileByIndex(0); AddMessage(VarToStr(v));
  AddMessage('after');
end;
end.
