{ parity probe: x-222 }
unit P_x_222;

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
  v := Assigned(0); AddMessage(VarToStr(v));
  AddMessage('after');
end;
end.
