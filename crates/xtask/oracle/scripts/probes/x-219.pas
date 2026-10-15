{ parity probe: x-219 }
unit P_x_219;

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
  v := Assigned(5); AddMessage(VarToStr(v));
  AddMessage('after');
end;
end.
