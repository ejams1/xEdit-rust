{ parity probe: x-172 }
unit P_x_172;

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
  v := Null; v := not v; AddMessage(VarToStr(v));
  AddMessage('after');
end;
end.
