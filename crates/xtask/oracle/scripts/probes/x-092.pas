{ parity probe: x-092 }
unit P_x_092;

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
  v := True; AddMessage(v);
  AddMessage('after');
end;
end.
