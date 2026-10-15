{ parity probe: x-320 }
unit P_x_320;

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
  sl.Free; sl.Free;
  AddMessage('after');
end;
end.
