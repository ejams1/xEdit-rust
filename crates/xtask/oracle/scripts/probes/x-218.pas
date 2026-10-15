{ parity probe: x-218 }
unit P_x_218;

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
  v := Assigned(nil); AddMessage(VarToStr(v));
  AddMessage('after');
end;
end.
