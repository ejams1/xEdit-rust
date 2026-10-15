{ parity probe: x-065 }
unit P_x_065;

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
  v := Name(nil);
  AddMessage('after');
end;
end.
