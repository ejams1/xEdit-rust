{ parity probe: x-229 }
unit P_x_229;

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
  v := FileByIndex(0); v := v = 0; AddMessage(VarToStr(v));
  AddMessage('after');
end;
end.
