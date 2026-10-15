{ parity probe: x-093 }
unit P_x_093;

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
  v := 1.5; AddMessage(v);
  AddMessage('after');
end;
end.
