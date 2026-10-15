{ parity probe: x-120 }
unit P_x_120;

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
  s := 5 + 5; AddMessage(s);
  AddMessage('after');
end;
end.
