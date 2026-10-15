{ parity probe: x-165 }
unit P_x_165;

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
  v := Unassigned; v := v = 0; AddMessage(VarToStr(v));
  AddMessage('after');
end;
end.
