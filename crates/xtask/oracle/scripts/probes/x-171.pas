{ parity probe: x-171 }
unit P_x_171;

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
  v := Unassigned; v := not v; AddMessage(VarToStr(v));
  AddMessage('after');
end;
end.
