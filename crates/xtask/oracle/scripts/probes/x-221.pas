{ parity probe: x-221 }
unit P_x_221;

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
  v := Assigned(''); AddMessage(VarToStr(v));
  AddMessage('after');
end;
end.
